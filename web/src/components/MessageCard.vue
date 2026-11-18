<script setup lang="ts">
import { shallowRef } from 'vue'
import { Check, Copy, CornerDownRight, Pencil, QrCode, Trash2, X } from '@lucide/vue'
import { api, errorText, messageSchema, request, type Message } from '../api'
import { copyText, formatDate } from '../format'
import AttachmentCard from './AttachmentCard.vue'
import TextQr from './TextQr.vue'

const { message } = defineProps<{ message: Message }>()
const emit = defineEmits<{ reply: [message: Message]; updated: [message: Message]; removed: [id: number] }>()
const copied = shallowRef(false)
const manualCopy = shallowRef(false)
const qr = shallowRef(false)
const editing = shallowRef(false)
const draft = shallowRef('')
const deleting = shallowRef(false)
const busy = shallowRef(false)
const error = shallowRef('')
async function copy() { copied.value = await copyText(message.text); manualCopy.value = !copied.value }
function selectText(event: FocusEvent) {
  if (event.target instanceof HTMLTextAreaElement) event.target.select()
}
function edit() { draft.value = message.text; editing.value = true; error.value = '' }
async function save() {
  busy.value = true; error.value = ''
  try {
    const updated = await api(`/messages/${message.id}`, messageSchema, { method: 'PATCH', body: JSON.stringify({ text: draft.value }) })
    emit('updated', updated); editing.value = false
  } catch (cause) { error.value = errorText(cause) }
  finally { busy.value = false }
}
async function remove() {
  busy.value = true; error.value = ''
  try { await request(`/messages/${message.id}`, { method: 'DELETE' }); emit('removed', message.id) }
  catch (cause) { error.value = errorText(cause) }
  finally { busy.value = false }
}
</script>

<template>
  <article class="message-card" :data-message-id="message.id">
    <div class="message-meta"><span class="message-marker" /><time :datetime="new Date(message.created_at).toISOString()">{{ formatDate(message.created_at) }}</time><span v-if="message.updated_at > message.created_at" class="edited">已编辑</span><span class="message-id">#{{ message.id }}</span></div>
    <div v-if="message.reply_to" class="reply-quote"><CornerDownRight :size="15" /><span>回复 #{{ message.reply_to }} · {{ message.reply_excerpt || '附件消息' }}</span></div>
    <template v-if="editing"><textarea v-model="draft" class="edit-field" aria-label="编辑消息" rows="4" :disabled="busy" /><div class="edit-actions"><button class="text-button" :disabled="busy" @click="editing = false">取消</button><button class="text-button accent" :disabled="busy" @click="save">保存修改</button></div></template>
    <p v-else-if="message.text" class="message-text">{{ message.text }}</p>
    <div v-if="message.attachments.length" class="attachments"><AttachmentCard v-for="file in message.attachments" :key="file.id" :file="file" /></div>
    <footer class="message-actions">
      <button v-if="message.text" class="action-button" @click="copy"><Check v-if="copied" :size="14" /><Copy v-else :size="14" /><span>{{ copied ? '已复制' : '复制' }}</span></button>
      <button v-if="message.text" class="action-button" @click="qr = true"><QrCode :size="14" /><span>二维码</span></button>
      <button class="action-button" @click="emit('reply', message)"><CornerDownRight :size="14" /><span>回复</span></button>
      <span class="action-spacer" />
      <button class="icon-button small" aria-label="编辑消息" @click="edit"><Pencil :size="14" /></button>
      <button class="icon-button small" aria-label="删除消息" @click="deleting = !deleting"><Trash2 :size="14" /></button>
    </footer>
    <div v-if="manualCopy" class="manual-copy"><span>请选中文本后复制</span><button class="icon-button" aria-label="关闭复制文本" @click="manualCopy = false"><X :size="15" /></button><textarea :value="message.text" readonly aria-label="可复制的消息文本" @focus="selectText" /></div>
    <div v-if="deleting" class="delete-confirm"><span>删除这条消息及其附件？</span><button class="text-button" :disabled="busy" @click="deleting = false">取消</button><button class="text-button danger" :disabled="busy" @click="remove">确认删除</button></div>
    <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
    <TextQr v-if="qr" :text="message.text" @close="qr = false" />
  </article>
</template>

<style scoped>
.message-card { padding: 21px 24px 12px; background: var(--surface); border: 1px solid var(--line); border-radius: 15px; }
.message-meta { display: flex; align-items: center; gap: 8px; font-size: 11px; color: var(--muted); }
.message-marker { width: 5px; height: 5px; border-radius: 50%; background: var(--accent); }
.message-id { margin-left: auto; font-family: ui-monospace, Consolas, monospace; color: var(--faint); }
.edited { color: var(--faint); }
.message-text { white-space: pre-wrap; overflow-wrap: anywhere; line-height: 1.85; font-size: 14px; margin: 15px 0 17px; tab-size: 4; }
.attachments { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin: 16px 0; }
.attachments > :only-child { grid-column: 1 / -1; }
.message-actions { display: flex; align-items: center; gap: 14px; }
.action-button { display: flex; align-items: center; gap: 5px; border: 0; padding: 8px 0; color: var(--muted); background: transparent; font-size: 11px; }
.action-button:hover { color: var(--accent); }
.action-spacer { flex: 1; }
.small { width: 28px; height: 28px; }
.reply-quote { display: flex; gap: 7px; align-items: center; padding: 9px 10px; background: var(--canvas); border-left: 2px solid var(--line-strong); border-radius: 0 6px 6px 0; color: var(--muted); font-size: 12px; margin-top: 15px; }
.reply-quote span { min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.edit-field { width: 100%; margin-top: 15px; padding: 12px; border: 1px solid var(--line-strong); border-radius: 8px; font: inherit; background: var(--canvas); color: var(--ink); resize: vertical; }
.edit-actions { display: flex; justify-content: flex-end; gap: 10px; }
.delete-confirm { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; border-top: 1px solid var(--line); padding-top: 8px; margin-top: 8px; font-size: 12px; }
.delete-confirm > span { margin-right: auto; }
.manual-copy { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; font-size: 12px; color: var(--muted); }
.manual-copy textarea { width: 100%; padding: 10px; font: inherit; background: var(--canvas); color: var(--ink); border: 1px solid var(--line); }
@media (max-width: 600px) { .message-card { padding: 17px 16px 10px; } .attachments { grid-template-columns: minmax(0, 1fr); } .message-actions { gap: 13px; } }
</style>
