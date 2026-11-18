import { computed, markRaw, onBeforeUnmount, ref } from 'vue'
import { ApiError, api, attachmentSchema, configSchema, errorText, request, uniqueId, uploadSchema, type Attachment } from './api'

type UploadStatus =
  | { kind: 'queued' | 'uploading' | 'paused' | 'hashing' }
  | { kind: 'error'; message: string }
  | { kind: 'ready'; attachment: Attachment }

export interface UploadTask {
  key: string
  file: File
  serverId: string | null
  received: number
  status: UploadStatus
}

export function useUploads() {
  const tasks = ref<UploadTask[]>([])
  const active = new Map<string, AbortController>()
  let chunkSize: number | undefined
  let disposed = false
  const ready = computed(() => tasks.value.every(task => task.status.kind === 'ready'))
  const attachments = computed(() => tasks.value.flatMap(task => task.status.kind === 'ready' ? [task.status.attachment] : []))

  async function run(task: UploadTask, controller: AbortController) {
    const signal = controller.signal
    try {
      chunkSize ??= (await api('/config', configSchema, { signal })).chunk_size
      task.status = { kind: 'uploading' }
      if (!task.serverId) {
        const upload = await api('/uploads', uploadSchema, {
          method: 'POST', body: JSON.stringify({ name: task.file.name, size: task.file.size }), signal,
        })
        task.serverId = upload.id
      }
      const path = `/uploads/${task.serverId}`
      let failures = 0
      for (;;) {
        signal.throwIfAborted()
        try {
          const current = await api(path, uploadSchema, { signal })
          task.received = current.received
          if (current.sha256 !== null) {
            task.status = { kind: 'ready', attachment: attachmentSchema.parse(current) }
            return
          }
          while (task.received < task.file.size) {
            const end = Math.min(task.received + chunkSize, task.file.size)
            const result = await api(path, uploadSchema, {
              method: 'PATCH', headers: { 'upload-offset': String(task.received) },
              body: task.file.slice(task.received, end), signal,
            })
            task.received = result.received
            failures = 0
          }
          task.status = { kind: 'hashing' }
          const completed = await api(`${path}/complete`, uploadSchema, { method: 'POST', signal })
          task.status = { kind: 'ready', attachment: attachmentSchema.parse(completed) }
          return
        } catch (cause) {
          if (signal.aborted) throw cause
          if (cause instanceof ApiError && cause.status >= 400 && cause.status < 500 && cause.status !== 409) throw cause
          if (++failures > 3) throw cause
          await new Promise<void>((resolve, reject) => {
            const timer = setTimeout(() => { signal.removeEventListener('abort', cancel); resolve() }, 700 * 2 ** failures)
            function cancel() { clearTimeout(timer); reject(new DOMException('Aborted', 'AbortError')) }
            signal.addEventListener('abort', cancel, { once: true })
          })
          task.status = { kind: 'uploading' }
        }
      }
    } catch (cause) {
      if (!signal.aborted) task.status = { kind: 'error', message: errorText(cause) }
    } finally {
      active.delete(task.key)
      pump()
    }
  }

  function pump() {
    if (disposed) return
    for (const task of tasks.value) {
      if (active.size >= 2) break
      if (task.status.kind !== 'queued' || active.has(task.key)) continue
      const controller = new AbortController()
      active.set(task.key, controller)
      void run(task, controller)
    }
  }
  function add(files: File[]) {
    tasks.value.push(...files.map(file => ({
      key: uniqueId(), file: markRaw(file), serverId: null, received: 0, status: { kind: 'queued' } satisfies UploadStatus,
    })))
    pump()
  }
  function pause(task: UploadTask) { task.status = { kind: 'paused' }; active.get(task.key)?.abort() }
  function resume(task: UploadTask) { task.status = { kind: 'queued' }; pump() }
  async function remove(task: UploadTask) {
    active.get(task.key)?.abort()
    tasks.value = tasks.value.filter(item => item.key !== task.key)
    if (task.serverId) await request(`/uploads/${task.serverId}`, { method: 'DELETE' })
  }
  function sent() { tasks.value = [] }
  onBeforeUnmount(() => { disposed = true; for (const controller of active.values()) controller.abort() })
  return { tasks, ready, attachments, add, pause, resume, remove, sent }
}
