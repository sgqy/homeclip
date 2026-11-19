import { createSHA256 } from 'hash-wasm'
import type { HashMessage } from './fileHash'

function send(message: HashMessage) { self.postMessage(message) }

self.onmessage = async (event: MessageEvent<unknown>) => {
  try {
    const file = event.data
    if (!(file instanceof File)) throw new Error('无法读取所选文件')
    const hasher = await createSHA256()
    hasher.init()
    const chunkSize = 8 * 1024 * 1024
    for (let offset = 0; offset < file.size; offset += chunkSize) {
      const end = Math.min(offset + chunkSize, file.size)
      hasher.update(new Uint8Array(await file.slice(offset, end).arrayBuffer()))
      send({ kind: 'progress', processed: end })
    }
    send({ kind: 'done', sha256: hasher.digest('hex') })
  } catch (error) {
    send({ kind: 'error', message: error instanceof Error ? error.message : '文件摘要计算失败' })
  }
}
