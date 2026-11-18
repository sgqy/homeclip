import { onBeforeUnmount, onMounted, shallowRef, watch } from 'vue'
import { ApiError, api, changeSchema, errorText, messageSchema, pageSchema, type Message } from './api'

export function useFeed() {
  const messages = shallowRef<Message[]>([])
  const query = shallowRef('')
  const connection = shallowRef<'connecting' | 'online' | 'offline'>('connecting')
  const error = shallowRef('')
  const loading = shallowRef(true)
  const nextBefore = shallowRef<number | null>(null)
  const history = shallowRef<(number | null)[]>([null])
  const newCount = shallowRef(0)
  let source: EventSource | undefined
  let abort: AbortController | undefined
  let generation = 0
  let disposed = false
  let eventQueue = Promise.resolve()
  let retry: ReturnType<typeof setTimeout> | undefined

  function upsert(message: Message) {
    const exists = messages.value.some(item => item.id === message.id)
    const search = query.value.trim().toLowerCase()
    const matches = message.text.toLowerCase().includes(search) || message.attachments.some(file => file.name.toLowerCase().includes(search))
    const recent = history.value.length === 1
    const rest = messages.value.filter(item => item.id !== message.id).map(item =>
      item.reply_to === message.id ? { ...item, reply_excerpt: message.text.slice(0, 120) } : item)
    if (matches && (exists || recent)) {
      messages.value = [...rest, message].sort((a, b) => b.id - a.id).slice(0, 40)
      if (messages.value.length === 40) nextBefore.value = messages.value.at(-1)?.id ?? null
    } else {
      messages.value = rest
      if (!exists && matches && !recent) newCount.value++
    }
  }

  function remove(id: number) {
    messages.value = messages.value.filter(message => message.id !== id).map(message =>
      message.reply_to === id ? { ...message, reply_to: null, reply_excerpt: null } : message)
  }

  async function refresh() {
    if (disposed) return
    const current = ++generation
    clearTimeout(retry)
    abort?.abort()
    abort = new AbortController()
    source?.close()
    connection.value = 'connecting'
    loading.value = true
    error.value = ''
    try {
      const params = new URLSearchParams({ q: query.value.trim(), limit: '40' })
      const before = history.value.at(-1)
      if (before != null) params.set('before', String(before))
      const page = await api(`/messages?${params}`, pageSchema, { signal: abort.signal })
      if (current !== generation || disposed) return
      messages.value = page.messages
      nextBefore.value = page.next_before
      source = new EventSource(`/api/events?after=${page.cursor}`)
      source.onopen = () => { if (current === generation) connection.value = 'online' }
      source.onerror = () => { if (current === generation) connection.value = 'offline' }
      source.addEventListener('reset', () => { void refresh() })
      source.addEventListener('change', (event: MessageEvent<string>) => {
        eventQueue = eventQueue.then(async () => {
          if (current !== generation || disposed) return
          const data: unknown = JSON.parse(event.data)
          const change = changeSchema.parse(data)
          if (change.kind === 'delete') { remove(change.message_id); return }
          try {
            const message = await api(`/messages/${change.message_id}`, messageSchema)
            if (current === generation && !disposed) upsert(message)
          } catch (cause) {
            if (cause instanceof ApiError && cause.status === 404) remove(change.message_id)
            else throw cause
          }
        }).catch(() => {
          // EventSource has already advanced its cursor. Reload a snapshot if hydration fails.
          if (current === generation && !disposed) void refresh()
        })
      })
    } catch (cause) {
      if (current !== generation || disposed) return
      error.value = errorText(cause)
      connection.value = 'offline'
      retry = setTimeout(() => { void refresh() }, 3000)
    } finally {
      if (current === generation) loading.value = false
    }
  }

  function latest() { history.value = [null]; newCount.value = 0; void refresh() }
  function older() {
    if (nextBefore.value != null) { history.value = [...history.value, nextBefore.value]; void refresh() }
  }
  function newer() {
    if (history.value.length > 1) { history.value = history.value.slice(0, -1); void refresh() }
  }
  watch(query, (_, __, cleanup) => {
    const timer = setTimeout(latest, 250)
    cleanup(() => clearTimeout(timer))
  })
  function visibility() { if (document.visibilityState === 'visible') void refresh() }
  onMounted(() => { void refresh(); document.addEventListener('visibilitychange', visibility) })
  onBeforeUnmount(() => {
    disposed = true; generation++; source?.close(); abort?.abort(); clearTimeout(retry)
    document.removeEventListener('visibilitychange', visibility)
  })
  return { messages, query, connection, error, loading, nextBefore, history, newCount, latest, older, newer, upsert, remove }
}
