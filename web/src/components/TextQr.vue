<script setup lang="ts">
import { onMounted, shallowRef, useTemplateRef } from 'vue'
import { QrCode, X } from '@lucide/vue'
import QRCode from 'qrcode'

const { text } = defineProps<{ text: string }>()
const emit = defineEmits<{ close: [] }>()
const dialog = useTemplateRef<HTMLDialogElement>('dialog')
const image = shallowRef('')
const error = shallowRef('')
const dense = shallowRef(false)
onMounted(async () => {
  dialog.value?.showModal()
  try {
    // 7089 numeric digits is the largest single QR payload. Other modes fit fewer characters.
    if (text.length > 7089) throw new Error('too long')
    let correction: 'M' | 'L' = 'M'
    let code: ReturnType<typeof QRCode.create>
    try { code = QRCode.create(text, { errorCorrectionLevel: correction }) }
    catch { correction = 'L'; code = QRCode.create(text, { errorCorrectionLevel: correction }) }
    dense.value = code.version > 15
    image.value = await QRCode.toDataURL(text, { errorCorrectionLevel: correction, width: dense.value ? 900 : 560, margin: 3 })
  } catch { error.value = '这条消息超过单个二维码的容量，请在手机上打开共享空间查看。' }
})
</script>

<template>
  <dialog ref="dialog" class="modal qr-modal" :class="{ dense }" aria-label="消息二维码" @cancel.prevent="emit('close')" @click="event => { if (event.target === dialog) emit('close') }">
    <header class="modal-header"><QrCode :size="19" /><strong>扫码带走这段文字</strong><button class="icon-button" aria-label="关闭二维码" @click="emit('close')"><X :size="20" /></button></header>
    <div class="qr-content"><img v-if="image" class="qr-image" :src="image" alt="包含消息原文的二维码" /><p v-if="error" class="inline-error" role="alert">{{ error }}</p><p v-else class="qr-hint">用手机扫码，然后选择复制。<br />二维码里是原文，离线也能识别。</p><pre class="qr-text">{{ text }}</pre></div>
  </dialog>
</template>

<style scoped>
.qr-modal { width: min(440px, calc(100vw - 32px)); }
.modal-header strong { flex: 1; font-size: 14px; }
.qr-content { padding: 10px 24px 24px; text-align: center; }
.qr-image { display: block; width: 100%; max-width: 320px; margin: auto; }
.qr-modal.dense { width: min(680px, calc(100vw - 32px)); }
.dense .qr-image { max-width: 600px; }
.qr-hint { color: var(--muted); line-height: 1.8; font-size: 12px; margin: 0 0 18px; }
.qr-text { max-height: 120px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; text-align: left; font: 12px/1.65 inherit; padding: 12px; margin: 0; background: var(--canvas); border-radius: 8px; }
</style>
