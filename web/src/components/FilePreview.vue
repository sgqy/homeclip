<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, shallowRef, useTemplateRef } from 'vue'
import { ArrowRight, Download, FileQuestion, RotateCcw, X } from '@lucide/vue'
import { errorText, fileUrl, type Attachment } from '../api'
import { formatSize } from '../format'

const { file } = defineProps<{ file: Attachment }>()
const emit = defineEmits<{ close: [] }>()
const dialog = useTemplateRef<HTMLDialogElement>('dialog')
const url = computed(() => fileUrl(file))
const kind = computed(() => {
  if (file.mime.startsWith('image/')) return 'image'
  if (file.mime.startsWith('video/')) return 'video'
  if (file.mime.startsWith('audio/')) return 'audio'
  const extension = file.name.split('.').at(-1)?.toLowerCase() ?? ''
  if (file.mime.startsWith('text/') || ['txt', 'md', 'json', 'yaml', 'yml', 'toml', 'ini', 'conf', 'log', 'csv', 'tsv', 'rs', 'js', 'ts', 'tsx', 'jsx', 'css', 'html', 'xml', 'sql', 'sh', 'bat', 'ps1', 'py', 'go', 'c', 'h', 'cpp', 'srt', 'vtt', 'env', 'gitignore'].includes(extension)) return 'text'
  return 'other'
})
const text = shallowRef('')
const encoding = shallowRef('utf-8')
const offset = shallowRef(0)
const start = shallowRef(0)
const loading = shallowRef(false)
const error = shallowRef('')
let decoder = new TextDecoder()
let controller: AbortController | undefined
let generation = 0

async function next() {
  if (loading.value || offset.value >= file.size) return
  const current = ++generation
  controller?.abort(); controller = new AbortController()
  loading.value = true; error.value = ''
  const from = offset.value
  const end = Math.min(from + 64 * 1024, file.size)
  try {
    const response = await fetch(url.value, { headers: { Range: `bytes=${from}-${end - 1}` }, signal: controller.signal })
    if (!response.ok || (response.status !== 206 && file.size > 64 * 1024)) {
      await response.body?.cancel()
      throw new Error('无法按段读取此文件，请下载后查看')
    }
    const bytes = await response.arrayBuffer()
    if (current !== generation) return
    start.value = from; offset.value = end
    text.value = decoder.decode(bytes, { stream: end < file.size })
  } catch (cause) { if (current === generation) error.value = errorText(cause) }
  finally { if (current === generation) loading.value = false }
}
function restart() {
  generation++; controller?.abort(); decoder = new TextDecoder(encoding.value)
  offset.value = 0; start.value = 0; loading.value = false; text.value = ''; error.value = ''
  void next()
}
function backdrop(event: MouseEvent) { if (event.target === dialog.value) emit('close') }
onMounted(() => { dialog.value?.showModal(); if (kind.value === 'text') void next() })
onBeforeUnmount(() => { generation++; controller?.abort() })
</script>

<template>
  <dialog ref="dialog" class="modal preview-modal" aria-label="文件预览" @cancel.prevent="emit('close')" @click="backdrop">
    <header class="modal-header">
      <div class="preview-title"><strong>{{ file.name }}</strong><span>{{ formatSize(file.size) }} · 原文件</span></div>
      <a class="icon-button" :href="fileUrl(file, true)" aria-label="保存原文件"><Download :size="19" /></a>
      <button class="icon-button" aria-label="关闭预览" @click="emit('close')"><X :size="21" /></button>
    </header>
    <div class="preview-content" :class="`preview-${kind}`">
      <img v-if="kind === 'image'" class="original-image" :src="url" :alt="file.name" @error="error = '浏览器无法显示此图片，可以保存原文件'" />
      <video v-else-if="kind === 'video'" class="original-video" :src="url" controls playsinline preload="metadata" @error="error = '浏览器无法播放此视频，可以保存原文件'" />
      <audio v-else-if="kind === 'audio'" :src="url" controls preload="metadata" @error="error = '浏览器无法播放此音频，可以保存原文件'" />
      <template v-else-if="kind === 'text'">
        <div class="text-toolbar"><select v-model="encoding" aria-label="文本编码" @change="restart"><option value="utf-8">UTF-8</option><option value="gb18030">GB18030</option><option value="utf-16le">UTF-16 LE</option><option value="utf-16be">UTF-16 BE</option></select><span>{{ formatSize(start) }} – {{ formatSize(offset) }} / {{ formatSize(file.size) }}</span></div>
        <pre class="text-preview">{{ text || (loading ? '读取中…' : '（空内容）') }}</pre>
        <div class="text-actions"><button class="text-button" :disabled="loading" @click="restart"><RotateCcw :size="15" />从头查看</button><button class="text-button" :disabled="loading || offset >= file.size" @click="next">{{ loading ? '读取中…' : '下一段' }}<ArrowRight :size="15" /></button></div>
      </template>
      <div v-else class="unsupported"><FileQuestion :size="42" stroke-width="1.3" /><p>这个文件可以直接保存</p><span>浏览器没有适用的预览方式</span><a class="primary-link" :href="fileUrl(file, true)"><Download :size="17" />保存原文件</a></div>
      <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
    </div>
  </dialog>
</template>

<style scoped>
.preview-modal { width: min(960px, calc(100vw - 32px)); }
.preview-title { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
.preview-title strong { font-size: 14px; overflow-wrap: anywhere; }
.preview-title span { color: var(--muted); font-size: 12px; }
.preview-content { padding: 22px; }
.preview-image, .preview-video, .preview-audio { text-align: center; background: var(--canvas); }
.original-image, .original-video { max-width: 100%; max-height: 70vh; object-fit: contain; display: block; margin: auto; }
.original-video { width: 100%; }
.preview-audio { padding: 80px 20px; }
.preview-audio audio { max-width: 100%; }
.text-toolbar, .text-actions { display: flex; justify-content: space-between; align-items: center; gap: 12px; font-size: 12px; color: var(--muted); }
.text-toolbar select { background: var(--surface); color: var(--ink); border: 1px solid var(--line); border-radius: 6px; padding: 6px; }
.text-preview { min-height: 200px; max-height: 55vh; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; margin: 14px 0; padding: 16px; border-radius: 8px; background: var(--canvas); font: 13px/1.65 ui-monospace, Consolas, monospace; text-align: left; }
.unsupported { min-height: 260px; display: flex; flex-direction: column; align-items: center; justify-content: center; color: var(--muted); gap: 9px; }
.unsupported p { color: var(--ink); margin: 6px 0 0; }
.unsupported span { font-size: 13px; }
.primary-link { display: inline-flex; gap: 8px; align-items: center; padding: 10px 16px; margin-top: 15px; background: var(--accent); color: white; border-radius: 9px; text-decoration: none; font-size: 13px; }
@media (max-width: 600px) { .preview-content { padding: 12px; } .text-toolbar { flex-wrap: wrap; } }
</style>
