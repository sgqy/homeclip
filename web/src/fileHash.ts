import { z } from 'zod'

const hashMessageSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('progress'), processed: z.number().nonnegative() }),
  z.object({ kind: z.literal('done'), sha256: z.string().regex(/^[a-f0-9]{64}$/) }),
  z.object({ kind: z.literal('error'), message: z.string() }),
])
export type HashMessage = z.infer<typeof hashMessageSchema>

export function hashFile({ file, signal, onProgress }: {
  file: File
  signal: AbortSignal
  onProgress: (processed: number) => void
}): Promise<string> {
  return new Promise((resolve, reject) => {
    signal.throwIfAborted()
    const worker = new Worker(new URL('./fileHash.worker.ts', import.meta.url), { type: 'module' })
    function cleanup() { signal.removeEventListener('abort', cancel); worker.terminate() }
    function fail(error: unknown) { cleanup(); reject(error) }
    function cancel() { fail(signal.reason) }
    worker.onmessage = (event: MessageEvent<unknown>) => {
      const parsed = hashMessageSchema.safeParse(event.data)
      if (!parsed.success) { fail(new Error('无法读取文件摘要计算结果')); return }
      const message = parsed.data
      switch (message.kind) {
        case 'progress': onProgress(message.processed); break
        case 'done': cleanup(); resolve(message.sha256); break
        case 'error': fail(new Error(message.message)); break
      }
    }
    worker.onerror = () => fail(new Error('文件摘要计算失败，请重试'))
    worker.onmessageerror = () => fail(new Error('无法读取文件摘要计算结果'))
    signal.addEventListener('abort', cancel, { once: true })
    worker.postMessage(file)
  })
}
