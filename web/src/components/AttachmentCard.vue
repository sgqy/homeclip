<script setup lang="ts">
import { computed, shallowRef } from 'vue'
import { Check, Copy, Download, File, FileText, Image, Info, Music, Video } from '@lucide/vue'
import { fileUrl, type Attachment } from '../api'
import { copyText, formatSize } from '../format'
import FilePreview from './FilePreview.vue'

const { file } = defineProps<{ file: Attachment }>()
const preview = shallowRef(false)
const details = shallowRef(false)
const copied = shallowRef(false)
const icon = computed(() => file.mime.startsWith('image/') ? Image : file.mime.startsWith('video/') ? Video : file.mime.startsWith('audio/') ? Music : file.mime.startsWith('text/') ? FileText : File)
const extension = computed(() => file.name.includes('.') ? file.name.split('.').at(-1)?.toUpperCase().slice(0, 8) : 'FILE')
async function copyHash() { copied.value = await copyText(file.sha256) }
</script>

<template>
  <div class="attachment-card">
    <div class="attachment-row">
      <button class="attachment-open" :aria-label="`预览 ${file.name}`" @click="preview = true">
        <span class="file-icon"><component :is="icon" :size="22" stroke-width="1.6" /></span>
        <span class="file-label"><span class="file-name">{{ file.name }}</span><span class="file-meta">{{ extension }}<span class="dot">·</span>{{ formatSize(file.size) }}</span></span>
      </button>
      <button class="icon-button" :aria-label="`${file.name} 文件信息`" :aria-expanded="details" @click="details = !details"><Info :size="17" /></button>
      <a class="icon-button" :href="fileUrl(file, true)" :aria-label="`保存 ${file.name}`"><Download :size="18" /></a>
    </div>
    <div v-if="details" class="file-details"><span class="hash-label">SHA-256<button class="icon-button" aria-label="复制 SHA-256" @click="copyHash"><Check v-if="copied" :size="14" /><Copy v-else :size="14" /></button></span><code>{{ file.sha256 }}</code><span>{{ file.size.toLocaleString() }} 字节 · {{ file.mime }}</span></div>
    <FilePreview v-if="preview" :file="file" @close="preview = false" />
  </div>
</template>

<style scoped>
.attachment-card { border: 1px solid var(--line); border-radius: 11px; background: var(--surface); overflow: hidden; }
.attachment-row { display: flex; align-items: center; gap: 3px; padding: 5px 9px 5px 5px; }
.attachment-open { display: flex; align-items: center; gap: 12px; flex: 1; min-width: 0; text-align: left; border: 0; background: none; color: inherit; padding: 9px; border-radius: 8px; }
.attachment-open:hover { background: var(--canvas); }
.file-icon { display: grid; place-items: center; color: var(--accent); background: var(--accent-soft); height: 42px; width: 38px; border-radius: 9px; flex-shrink: 0; }
.file-label { min-width: 0; display: flex; flex-direction: column; gap: 5px; }
.file-name { font-size: 13px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.file-meta { font-size: 10px; color: var(--muted); letter-spacing: .4px; }
.dot { margin: 0 7px; }
.file-details { border-top: 1px solid var(--line); padding: 12px 14px; display: flex; flex-direction: column; gap: 6px; font-size: 11px; color: var(--muted); }
.hash-label { display: flex; align-items: center; justify-content: space-between; letter-spacing: .8px; }
.file-details code { font-size: 11px; line-height: 1.6; overflow-wrap: anywhere; color: var(--ink); user-select: all; }
</style>
