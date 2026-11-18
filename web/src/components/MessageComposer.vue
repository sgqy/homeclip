<script setup lang="ts">
import { computed, shallowRef, useTemplateRef, watch } from 'vue'
import { ArrowUp, Check, FileUp, Paperclip, Pause, Play, RotateCcw, X } from '@lucide/vue'
import { api, errorText, messageSchema, uniqueId, type Message } from '../api'
import { formatSize } from '../format'
import { useUploads, type UploadTask } from '../useUploads'

const { reply } = defineProps<{ reply: Pick<Message, 'id' | 'text'> | null }>()
const emit = defineEmits<{ sent: [message: Message]; cancelReply: [] }>()
const uploads = useUploads()
const { tasks, ready, attachments } = uploads
const field = useTemplateRef<HTMLTextAreaElement>('field')
const picker = useTemplateRef<HTMLInputElement>('picker')
function initialDraft() { try { return localStorage.getItem('homeclip.draft') ?? '' } catch { return '' } }
const draft = shallowRef(initialDraft())
const sending = shallowRef(false)
const error = shallowRef('')
const dragDepth = shallowRef(0)
const canSend = computed(() => !sending.value && ready.value && (draft.value.trim().length > 0 || tasks.value.length > 0))
let pending: { signature: string; clientId: string } | undefined
watch(draft, value => { try { localStorage.setItem('homeclip.draft', value) } catch { /* Draft persistence is optional. */ } })
watch(() => reply, () => field.value?.focus())

async function send() {
  if (!canSend.value) return
  sending.value = true
  error.value = ''
  const content = { text: draft.value, attachments: attachments.value.map(file => file.id), reply_to: reply?.id ?? null }
  const signature = JSON.stringify(content)
  if (pending?.signature !== signature) pending = { signature, clientId: uniqueId() }
  try {
    const message = await api('/messages', messageSchema, { method: 'POST', body: JSON.stringify({ ...content, client_id: pending.clientId }) })
    draft.value = ''; uploads.sent(); pending = undefined
    emit('sent', message); emit('cancelReply')
  } catch (cause) { error.value = errorText(cause) }
  finally { sending.value = false }
}
function choose(event: Event) {
  if (event.target instanceof HTMLInputElement && event.target.files) {
    uploads.add(Array.from(event.target.files)); event.target.value = ''
  }
}
function drop(event: DragEvent) {
  dragDepth.value = 0
  if (!sending.value && event.dataTransfer) uploads.add(Array.from(event.dataTransfer.files))
}
function paste(event: ClipboardEvent) {
  if (event.clipboardData?.files.length && !sending.value) {
    event.preventDefault(); uploads.add(Array.from(event.clipboardData.files))
  }
}
function shortcut(event: KeyboardEvent) {
  if (event.key === 'Enter' && (event.ctrlKey || event.metaKey) && !event.isComposing) { event.preventDefault(); void send() }
}
async function remove(task: UploadTask) {
  try { await uploads.remove(task) } catch (cause) { error.value = errorText(cause) }
}
function status(task: UploadTask) {
  switch (task.status.kind) {
    case 'queued': return '等待上传'
    case 'paused': return '已暂停'
    case 'uploading': return `${formatSize(task.received)} / ${formatSize(task.file.size)}`
    case 'hashing': return '校验原文件…'
    case 'ready': return `${formatSize(task.file.size)} · 已就绪`
    case 'error': return task.status.message
  }
}
</script>

<template>
  <section class="composer" :class="{ dragging: dragDepth > 0 }" aria-label="发送消息"
    @dragover.prevent @dragenter.prevent="dragDepth++" @dragleave.prevent="dragDepth = Math.max(0, dragDepth - 1)" @drop.prevent="drop">
    <div v-if="reply" class="reply-draft">
      <span>回复 #{{ reply.id }} <span class="reply-text">{{ reply.text || '附件消息' }}</span></span>
      <button class="icon-button" aria-label="取消回复" @click="emit('cancelReply')"><X :size="16" /></button>
    </div>
    <textarea ref="field" v-model="draft" class="composer-input" aria-label="消息内容" placeholder="写点什么，或把文件拖到这里…"
      :disabled="sending" rows="3" @paste="paste" @keydown="shortcut" />
    <div v-if="tasks.length" class="upload-list" aria-label="上传队列">
      <div v-for="task in tasks" :key="task.key" class="upload-item">
        <div class="upload-icon"><Check v-if="task.status.kind === 'ready'" :size="19" /><FileUp v-else :size="19" /></div>
        <div class="upload-info">
          <span class="upload-name">{{ task.file.name }}</span>
          <span class="upload-status" :class="{ failed: task.status.kind === 'error' }">{{ status(task) }}</span>
          <progress v-if="task.status.kind !== 'ready'" :value="task.received" :max="task.file.size || 1" :aria-label="`${task.file.name} 上传进度`" />
        </div>
        <button v-if="task.status.kind === 'uploading' || task.status.kind === 'queued'" class="icon-button" :aria-label="`暂停 ${task.file.name}`" @click="uploads.pause(task)"><Pause :size="16" /></button>
        <button v-if="task.status.kind === 'paused' || task.status.kind === 'error'" class="icon-button" :aria-label="`继续 ${task.file.name}`" @click="uploads.resume(task)"><Play v-if="task.status.kind === 'paused'" :size="16" /><RotateCcw v-else :size="16" /></button>
        <button class="icon-button" :disabled="sending" :aria-label="`移除 ${task.file.name}`" @click="remove(task)"><X :size="16" /></button>
      </div>
    </div>
    <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
    <div class="composer-footer">
      <input ref="picker" class="visually-hidden" type="file" multiple aria-label="选择文件" :disabled="sending" @change="choose" />
      <button class="attach-button" :disabled="sending" @click="picker?.click()"><Paperclip :size="18" /><span>添加文件</span></button>
      <span class="composer-hint">原文件传输 · 支持粘贴图片</span>
      <button class="send-button" :disabled="!canSend" @click="send"><span>{{ sending ? '发送中…' : '发送' }}</span><ArrowUp :size="18" /></button>
    </div>
    <div v-if="dragDepth > 0" class="drop-overlay"><FileUp :size="32" /><span>松开，添加到这条消息</span></div>
  </section>
</template>

<style scoped>
.composer { position: relative; border: 1px solid var(--line); background: var(--surface); border-radius: 18px; box-shadow: 0 5px 22px #29251e05; }
.composer-input { display: block; width: 100%; min-height: 122px; max-height: 45vh; padding: 22px 24px 12px; resize: vertical; border: 0; outline: 0; background: transparent; font: inherit; line-height: 1.75; color: var(--ink); }
.composer-input::placeholder { color: var(--faint); }
.composer-footer { display: flex; align-items: center; gap: 15px; padding: 12px 16px 14px; }
.attach-button { display: inline-flex; align-items: center; gap: 7px; padding: 9px; border: 0; background: none; color: var(--muted); }
.attach-button:hover { color: var(--accent); }
.composer-hint { color: var(--faint); font-size: 12px; }
.send-button { margin-left: auto; display: inline-flex; align-items: center; gap: 18px; border: 0; border-radius: 10px; padding: 10px 16px; background: var(--accent); color: white; font-weight: 600; }
.send-button:disabled { opacity: .4; }
.upload-list { display: grid; gap: 8px; padding: 0 20px 10px; }
.upload-item { display: flex; align-items: center; gap: 10px; min-width: 0; padding: 10px; border: 1px solid var(--line); border-radius: 10px; }
.upload-icon { color: var(--accent); background: var(--accent-soft); padding: 9px; border-radius: 8px; }
.upload-info { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
.upload-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
.upload-status { font-size: 11px; color: var(--muted); overflow-wrap: anywhere; }
.failed { color: var(--danger); }
progress { width: 100%; height: 3px; accent-color: var(--accent); }
.reply-draft { margin: 14px 20px 0; padding: 7px 10px; background: var(--accent-soft); color: var(--accent); border-left: 2px solid var(--accent); display: flex; align-items: center; justify-content: space-between; font-size: 12px; gap: 8px; }
.reply-draft > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.reply-text { color: var(--muted); margin-left: 10px; }
.inline-error { margin: 0 24px; }
.drop-overlay { pointer-events: none; position: absolute; inset: 0; border: 2px dashed var(--accent); border-radius: 18px; background: var(--surface-translucent); display: flex; flex-direction: column; gap: 12px; align-items: center; justify-content: center; color: var(--accent); }
@media (max-width: 600px) { .composer-input { padding: 18px 17px 8px; } .composer-hint { display: none; } .composer-footer { padding: 9px 12px 12px; } .upload-list { padding: 0 12px 8px; } }
</style>
