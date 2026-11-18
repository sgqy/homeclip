<script setup lang="ts">
import { computed, shallowRef, useTemplateRef } from 'vue'
import { ArrowLeft, ArrowRight, ArrowUpRight, CircleHelp, ClipboardList, Files, House, Inbox, Laptop, MessageSquare, RefreshCw, Search, Smartphone, Wifi, WifiOff, X } from '@lucide/vue'
import type { Message } from './api'
import { useFeed } from './useFeed'
import MessageCard from './components/MessageCard.vue'
import MessageComposer from './components/MessageComposer.vue'

const feed = useFeed()
const { messages, query, connection, error, loading, nextBefore, history, newCount } = feed
const reply = shallowRef<Message | null>(null)
const composer = useTemplateRef<HTMLElement>('composer')
const help = shallowRef(false)
const status = computed(() => connection.value === 'online' ? '实时连接' : connection.value === 'connecting' ? '连接中' : '正在重连')
function chooseReply(message: Message) {
  reply.value = message
  composer.value?.scrollIntoView({ behavior: 'smooth', block: 'center' })
}
function sent(message: Message) { query.value = ''; feed.latest(); feed.upsert(message) }
function home() { query.value = ''; feed.latest() }
function remove(id: number) { feed.remove(id); if (reply.value?.id === id) reply.value = null }
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar">
      <button class="brand" aria-label="Homeclip 首页" @click="home"><span class="brand-mark"><ClipboardList :size="23" stroke-width="1.8" /></span><span>homeclip<span class="brand-period">.</span></span></button>
      <div class="sidebar-label">家里的共享空间</div>
      <nav class="navigation" aria-label="主导航"><button class="nav-item selected" @click="home"><House :size="18" /><span>共享动态</span><span class="nav-dot" /></button></nav>
      <div class="sidebar-note"><span class="note-symbol">↗</span><p>一段文字，<br />一个文件，<br />换个屏幕继续。</p><span>都留在家里。</span></div>
      <div class="sidebar-bottom"><div class="device-pair"><Laptop :size="18" /><span class="device-line" /><Smartphone :size="16" /></div><span>PC & 手机 · 浏览器即客户端</span><button class="help-button" @click="help = !help"><CircleHelp :size="14" />使用说明</button></div>
    </aside>

    <main class="main-content">
      <header class="topbar"><div class="breadcrumb"><span>HOME</span><span class="breadcrumb-separator">/</span><span>共享空间</span></div><div class="connection" :class="connection" role="status"><span class="connection-dot" /><span>{{ status }}</span><Wifi v-if="connection === 'online'" :size="14" /><WifiOff v-else :size="14" /></div></header>
      <section class="intro"><div class="intro-label"><span />让设备之间，近一点</div><h1>随手放，随处取<span>。</span></h1><p>文字、照片、大文件。在家里的每一块屏幕上接着用。</p></section>

      <section v-if="help" class="help-panel"><div><strong>打开，就能一起用</strong><button class="icon-button" aria-label="关闭使用说明" @click="help = false"><X :size="17" /></button></div><p>发送的文字和附件会出现在所有打开的客户端上。粘贴图片、拖入文件，或点击“添加文件”。Ctrl / ⌘ + Enter 可以发送。</p><p>点击附件查看原文件，点击下载图标保存。文件信息里有 SHA-256。文字消息可以生成原文二维码，在手机扫码工具里复制。</p><p>上传中断可以继续。上传时请保持页面打开；刷新页面会丢失尚未发送的附件队列。文字草稿会保留在当前浏览器。</p></section>

      <div ref="composer"><MessageComposer :reply="reply" @sent="sent" @cancel-reply="reply = null" /></div>

      <section class="feed-section" aria-label="共享动态">
        <div class="feed-toolbar"><div class="feed-heading"><h2>{{ history.length > 1 ? '历史动态' : '共享动态' }}</h2><span v-if="!loading && messages.length" class="count-badge">{{ messages.length }}</span></div><label class="search-field"><Search :size="15" /><input v-model="query" type="search" placeholder="搜索文字或文件名" aria-label="搜索文字或文件名" /></label><button class="icon-button refresh" aria-label="刷新消息" :disabled="loading" @click="feed.latest"><RefreshCw :size="16" /></button></div>
        <button v-if="history.length > 1" class="latest-banner" @click="feed.latest"><ArrowLeft :size="15" />{{ newCount ? `${newCount} 条新动态，返回最新` : '返回最新动态' }}<ArrowUpRight :size="15" /></button>
        <p v-if="error" class="inline-error feed-error" role="alert">{{ error }} · 将自动重试</p>
        <div v-if="loading && !messages.length" class="loading-state" aria-label="加载中"><span /><span /><span /></div>
        <div v-else-if="!messages.length && !error" class="empty-state"><div class="empty-illustration"><div class="empty-card back"><Files :size="23" /></div><div class="empty-card front"><Inbox :size="31" stroke-width="1.4" /></div><span class="empty-spark">+</span></div><h3>{{ query ? '还没有找到相关内容' : '这里，等你放点东西' }}</h3><p>{{ query ? '换个词，或者试试文件名。' : '贴一段文字，传一张照片，让另一个设备接住。' }}</p><div v-if="!query" class="empty-features"><span><MessageSquare :size="13" />即时同步</span><i /><span><Files :size="13" />原文件直传</span></div></div>
        <div v-else class="message-list" :aria-busy="loading"><MessageCard v-for="message in messages" :key="message.id" :message="message" @reply="chooseReply" @updated="feed.upsert" @removed="remove" /></div>
        <div v-if="messages.length && (nextBefore !== null || history.length > 1)" class="pagination"><button class="text-button" :disabled="loading || history.length === 1" @click="feed.newer"><ArrowLeft :size="15" />较新动态</button><span>第 {{ history.length }} 页</span><button class="text-button" :disabled="loading || nextBefore === null" @click="feed.older">更早动态<ArrowRight :size="15" /></button></div>
      </section>
      <footer class="page-footer"><span>homeclip</span><span>一个家，一个共享空间。</span></footer>
    </main>
  </div>
</template>

<style scoped>
.app-shell { max-width: 1300px; margin: auto; display: grid; grid-template-columns: 232px minmax(0, 1fr); min-height: 100vh; }
.sidebar { position: sticky; top: 0; height: 100vh; border-right: 1px solid var(--line); padding: 40px 26px; display: flex; flex-direction: column; }
.brand { display: flex; align-items: center; gap: 11px; border: none; background: none; font-size: 25px; font-weight: 750; letter-spacing: -1px; color: var(--ink); padding: 0; }
.brand-mark { display: grid; place-items: center; width: 38px; height: 38px; border-radius: 12px; background: var(--accent); color: white; box-shadow: 0 4px 10px #e96a421c; }
.brand-period { color: var(--accent); }
.sidebar-label { margin: 44px 12px 15px; font-size: 10px; letter-spacing: 1.8px; color: var(--faint); }
.nav-item { display: flex; align-items: center; width: 100%; gap: 11px; padding: 13px 14px; border: 0; border-radius: 10px; background: var(--accent-soft); color: var(--accent); font-size: 13px; font-weight: 600; text-align: left; }
.nav-dot { width: 5px; height: 5px; background: var(--accent); border-radius: 50%; margin-left: auto; }
.sidebar-note { margin: 60px 14px 0; }
.note-symbol { display: inline-grid; place-items: center; height: 26px; width: 26px; font-size: 21px; color: var(--accent); border: 1px solid var(--line-strong); border-radius: 50%; transform: rotate(-8deg); }
.sidebar-note p { font-size: 20px; line-height: 1.8; letter-spacing: 1px; margin: 18px 0 10px; color: var(--muted); font-weight: 400; }
.sidebar-note > span:last-child { font-size: 11px; color: var(--faint); }
.sidebar-bottom { margin-top: auto; display: flex; flex-direction: column; gap: 15px; padding: 25px 12px 0; font-size: 10px; color: var(--faint); }
.device-pair { display: flex; align-items: center; gap: 8px; color: var(--muted); }
.device-line { width: 28px; border-top: 1px dashed var(--line-strong); }
.help-button { display: flex; gap: 6px; align-items: center; font-size: 11px; color: var(--muted); padding: 0; border: 0; background: none; }
.main-content { min-width: 0; padding: 38px 64px 20px; }
.topbar { display: flex; align-items: center; justify-content: space-between; gap: 15px; }
.breadcrumb { display: flex; gap: 14px; align-items: center; font-size: 11px; color: var(--muted); }
.breadcrumb > span:first-child { font-size: 10px; letter-spacing: 1.8px; color: var(--faint); }
.breadcrumb-separator { color: var(--line-strong); }
.connection { display: flex; align-items: center; gap: 7px; color: var(--muted); font-size: 10px; }
.connection-dot { width: 5px; height: 5px; border-radius: 50%; background: #c09548; }
.online .connection-dot { background: #6d9675; box-shadow: 0 0 0 3px #6d967512; }
.offline .connection-dot { background: var(--accent); }
.connection svg { margin-left: 2px; opacity: .65; }
.intro { margin: 49px 0 29px; }
.intro-label { display: flex; align-items: center; gap: 7px; font-size: 10px; letter-spacing: 1.7px; color: var(--muted); }
.intro-label span { display: block; width: 18px; height: 1px; background: var(--accent); }
.intro h1 { font-size: clamp(27px, 2.6vw, 36px); font-weight: 650; letter-spacing: -.8px; margin: 13px 0 10px; line-height: 1.4; }
.intro h1 span { color: var(--accent); }
.intro p { color: var(--muted); font-size: 12px; margin: 0; line-height: 1.8; }
.feed-section { margin-top: 34px; }
.feed-toolbar { display: flex; align-items: center; gap: 10px; margin-bottom: 17px; }
.feed-heading { display: flex; align-items: center; gap: 9px; flex: 1; }
.feed-heading h2 { font-size: 13px; font-weight: 600; margin: 0; white-space: nowrap; }
.count-badge { display: grid; place-items: center; border: 1px solid var(--line); min-width: 20px; height: 18px; border-radius: 5px; padding: 0 4px; font-size: 10px; color: var(--muted); }
.search-field { display: flex; align-items: center; gap: 8px; width: 220px; color: var(--faint); padding: 7px 10px; border: 1px solid transparent; border-radius: 8px; }
.search-field:focus-within { border-color: var(--line-strong); background: var(--surface); }
.search-field input { min-width: 0; width: 100%; border: 0; outline: 0; background: none; font-size: 11px; color: var(--ink); }
.search-field input::placeholder { color: var(--faint); }
.message-list { display: flex; flex-direction: column; gap: 12px; }
.empty-state { text-align: center; padding: 54px 20px 60px; border: 1px dashed var(--line-strong); border-radius: 15px; }
.empty-illustration { position: relative; height: 82px; width: 110px; margin: 0 auto 17px; }
.empty-card { position: absolute; width: 58px; height: 68px; border-radius: 12px; display: grid; place-items: center; border: 1px solid var(--line-strong); background: var(--surface); }
.back { left: 8px; top: 0; transform: rotate(-12deg); color: var(--faint); }
.front { right: 10px; bottom: 0; transform: rotate(9deg); color: var(--accent); background: var(--accent-soft); border-color: var(--accent-line); }
.empty-spark { position: absolute; right: -5px; top: 0; color: var(--accent); font-size: 18px; }
.empty-state h3 { font-size: 14px; font-weight: 500; margin: 0 0 9px; }
.empty-state p { font-size: 11px; color: var(--faint); line-height: 1.8; margin: 0; }
.empty-features { display: flex; justify-content: center; align-items: center; gap: 18px; margin-top: 24px; font-size: 10px; color: var(--muted); }
.empty-features span { display: inline-flex; align-items: center; gap: 5px; }
.empty-features i { width: 2px; height: 2px; background: var(--faint); border-radius: 50%; }
.pagination { display: flex; align-items: center; justify-content: space-between; margin-top: 20px; color: var(--faint); font-size: 11px; }
.latest-banner { display: flex; align-items: center; gap: 7px; border: 1px solid var(--accent-line); border-radius: 8px; background: var(--accent-soft); padding: 10px 13px; width: 100%; color: var(--accent); font-size: 12px; margin-bottom: 15px; }
.latest-banner svg:last-child { margin-left: auto; }
.loading-state { display: grid; gap: 12px; }
.loading-state span { height: 120px; border-radius: 15px; background: var(--surface); border: 1px solid var(--line); }
.help-panel { background: var(--surface); border: 1px solid var(--line); border-radius: 12px; padding: 16px 20px; margin-bottom: 20px; font-size: 12px; line-height: 1.9; color: var(--muted); }
.help-panel > div { display: flex; justify-content: space-between; align-items: center; color: var(--ink); }
.help-panel p { margin: 8px 0 0; }
.page-footer { display: flex; justify-content: space-between; margin-top: 42px; padding: 20px 0 10px; border-top: 1px solid var(--line); color: var(--faint); font-size: 10px; }
.page-footer span:first-child { font-weight: 600; letter-spacing: -.3px; }
@media (max-width: 1050px) { .app-shell { grid-template-columns: 200px minmax(0, 1fr); } .main-content { padding: 32px; } .sidebar { padding: 34px 20px; } }
@media (max-width: 760px) { .app-shell { display: block; } .sidebar { position: static; height: auto; border: 0; padding: 24px 22px 0; } .brand { font-size: 23px; } .brand-mark { width: 33px; height: 33px; border-radius: 10px; } .sidebar-label, .navigation, .sidebar-note, .sidebar-bottom { display: none; } .main-content { padding: 0 20px 16px; } .topbar { justify-content: flex-end; margin-top: -24px; height: 24px; } .breadcrumb { display: none; } .intro { margin: 35px 0 23px; } .intro h1 { font-size: 29px; } .intro p { font-size: 11px; } .feed-section { margin-top: 27px; } .search-field { width: 175px; padding-right: 0; } .feed-toolbar { gap: 5px; } .refresh { width: 27px; } .empty-state { padding: 42px 12px; } .page-footer { margin-top: 30px; } }
@media (max-width: 380px) { .main-content { padding-left: 13px; padding-right: 13px; } .sidebar { padding-left: 15px; } .search-field { width: 145px; } }
</style>
