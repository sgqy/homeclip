import { createHash, randomUUID } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import { expect, test } from '@playwright/test'
import jsQR from 'jsqr'
import { messageSchema, pageSchema } from '../src/api'

test.beforeEach(async ({ request }) => {
  for (;;) {
    const page = pageSchema.parse(await (await request.get('/api/messages')).json())
    if (!page.messages.length) break
    for (const message of page.messages) await request.delete(`/api/messages/${message.id}`)
  }
})

test('text synchronizes, replies and edits propagate, and QR carries the original text', async ({ page, context }, testInfo) => {
  const peer = await context.newPage()
  await page.goto('/')
  await peer.goto('/')
  await expect(page.getByRole('status')).toHaveText('实时连接')
  const text = '回家路上，记得带牛奶。\n https://example.com/家庭?a=1&b=2 '
  await page.getByRole('textbox', { name: '消息内容', exact: true }).fill(text)
  await page.getByRole('button', { name: '发送', exact: true }).click()
  await expect(peer.locator('.message-text')).toHaveText(text)
  await page.getByRole('button', { name: '二维码', exact: true }).click()
  await expect(page.getByRole('img', { name: '包含消息原文的二维码' })).toBeVisible()
  const pixels = await page.getByRole('img', { name: '包含消息原文的二维码' }).evaluate(element => {
    if (!(element instanceof HTMLImageElement)) throw new Error('QR image missing')
    const canvas = document.createElement('canvas')
    canvas.width = element.naturalWidth; canvas.height = element.naturalHeight
    const context = canvas.getContext('2d')
    if (!context) throw new Error('Canvas unavailable')
    context.drawImage(element, 0, 0)
    return { width: canvas.width, height: canvas.height, data: Array.from(context.getImageData(0, 0, canvas.width, canvas.height).data) }
  })
  expect(jsQR(new Uint8ClampedArray(pixels.data), pixels.width, pixels.height)?.data).toBe(text)
  await expect(page.locator('.qr-text')).toHaveText(text)
  await page.screenshot({ path: testInfo.outputPath('qr.png'), fullPage: true })
  await page.getByRole('button', { name: '关闭二维码' }).click()
  await peer.getByRole('button', { name: '回复', exact: true }).click()
  await peer.getByRole('textbox', { name: '消息内容', exact: true }).fill('收到，顺便带面包。')
  await peer.getByRole('button', { name: '发送', exact: true }).click()
  await expect(page.locator('.message-card')).toHaveCount(2)
  await expect(page.locator('.reply-quote')).toContainText('回家路上')
  const parentId = await page.locator('.message-card').filter({ has: page.locator('.message-text', { hasText: '回家路上' }) }).getAttribute('data-message-id')
  const parent = page.locator(`[data-message-id="${parentId}"]`)
  await parent.getByRole('button', { name: '编辑消息' }).click()
  await parent.getByRole('textbox', { name: '编辑消息' }).fill('清单已更新：牛奶、面包。')
  await parent.getByRole('button', { name: '保存修改' }).click()
  await expect(peer.locator('.message-text').filter({ hasText: '清单已更新' })).toBeVisible()
  await expect(peer.locator('.reply-quote')).toContainText('清单已更新')
  await page.screenshot({ path: testInfo.outputPath('messages.png'), fullPage: true })
  await parent.getByRole('button', { name: '删除消息' }).click()
  await parent.getByRole('button', { name: '确认删除' }).click()
  await expect(peer.locator('.message-card')).toHaveCount(1)
  await expect(peer.locator('.reply-quote')).toHaveCount(0)
})

test('a multi-chunk attachment previews by range and downloads byte-for-byte', async ({ page }, testInfo) => {
  test.setTimeout(90_000)
  const buffer = Buffer.alloc(8 * 1024 * 1024 + 123, 'a')
  buffer.write('家庭原始文件\n')
  const hash = createHash('sha256').update(buffer).digest('hex')
  await page.goto('/')
  await page.getByLabel('选择文件').setInputFiles({ name: '家庭记录.txt', mimeType: 'text/plain', buffer })
  await expect(page.getByRole('button', { name: '发送', exact: true })).toBeEnabled({ timeout: 60_000 })
  await page.getByRole('textbox', { name: '消息内容', exact: true }).fill('这份文件保持原样。')
  await page.getByRole('button', { name: '发送', exact: true }).click()
  await expect(page.getByRole('button', { name: '预览 家庭记录.txt' })).toBeVisible()
  await page.getByRole('button', { name: '家庭记录.txt 文件信息' }).click()
  await expect(page.locator('.file-details code')).toHaveText(hash)
  const rangeResponse = page.waitForResponse(response => response.url().includes('/content') && response.status() === 206)
  await page.getByRole('button', { name: '预览 家庭记录.txt' }).click()
  const response = await rangeResponse
  expect(response.headers()['content-range']).toBe(`bytes 0-65535/${buffer.length}`)
  await expect(page.locator('.text-preview')).toContainText('家庭原始文件')
  await page.getByRole('button', { name: '下一段' }).click()
  await expect(page.locator('.text-toolbar')).toContainText('64 KiB – 128 KiB')
  await page.getByRole('button', { name: '关闭预览' }).click()
  const downloadPromise = page.waitForEvent('download')
  await page.getByRole('link', { name: '保存 家庭记录.txt' }).click()
  const download = await downloadPromise
  expect(download.suggestedFilename()).toBe('家庭记录.txt')
  const saved = testInfo.outputPath('download.txt')
  await download.saveAs(saved)
  expect(createHash('sha256').update(await readFile(saved)).digest('hex')).toBe(hash)
})

test('image paste keeps the original file and layout fits the viewport', async ({ page }, testInfo) => {
  await page.goto('/')
  const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aRZkAAAAASUVORK5CYII='
  await page.getByRole('textbox', { name: '消息内容', exact: true }).evaluate((element, content) => {
    const transfer = new DataTransfer()
    transfer.items.add(new File([Uint8Array.from(atob(content), char => char.charCodeAt(0))], '粘贴图片.png', { type: 'image/png' }))
    element.dispatchEvent(new ClipboardEvent('paste', { clipboardData: transfer, bubbles: true, cancelable: true }))
  }, png)
  await expect(page.getByRole('button', { name: '发送', exact: true })).toBeEnabled()
  await page.getByRole('button', { name: '发送', exact: true }).click()
  await page.getByRole('button', { name: '预览 粘贴图片.png' }).click()
  const image = page.getByRole('img', { name: '粘贴图片.png', exact: true })
  await expect(image).toBeVisible()
  expect(await image.evaluate(element => element instanceof HTMLImageElement ? element.naturalWidth : 0)).toBe(1)
  await page.getByRole('button', { name: '关闭预览' }).click()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  await page.screenshot({ path: testInfo.outputPath('image-message.png'), fullPage: true })
})

test('history pages stay bounded, search finds filenames and reconnect catches up', async ({ page, request, context }) => {
  for (let index = 0; index < 43; index++) {
    const response = await request.post('/api/messages', { data: { client_id: randomUUID(), text: `历史消息 ${index}` } })
    expect(response.ok()).toBeTruthy()
  }
  await page.goto('/')
  await expect(page.locator('.message-card')).toHaveCount(40)
  await page.getByRole('button', { name: '更早动态' }).click()
  await expect(page.locator('.message-card')).toHaveCount(3)
  await page.getByRole('button', { name: '返回最新动态' }).click()
  await page.getByRole('searchbox').fill('历史消息 42')
  await expect(page.locator('.message-card')).toHaveCount(1)
  await page.getByRole('searchbox').fill('')
  await expect(page.locator('.message-card')).toHaveCount(40)
  await context.setOffline(true)
  const created = messageSchema.parse(await (await request.post('/api/messages', { data: { client_id: randomUUID(), text: '离线期间的新消息' } })).json())
  await context.setOffline(false)
  await expect(page.locator(`[data-message-id="${created.id}"]`)).toBeVisible({ timeout: 20_000 })
  await expect(page.locator('.message-card')).toHaveCount(40)
})

test('failed uploads can resume and long text gets a clear QR capacity result', async ({ page, context }) => {
  await page.goto('/')
  await expect(page.getByRole('status')).toHaveText('实时连接')
  await context.setOffline(true)
  await page.getByLabel('选择文件').setInputFiles({ name: '重试.txt', mimeType: 'text/plain', buffer: Buffer.from('可以继续') })
  await expect(page.getByRole('button', { name: '继续 重试.txt' })).toBeVisible()
  await context.setOffline(false)
  await page.getByRole('button', { name: '继续 重试.txt' }).click()
  await expect(page.getByRole('button', { name: '发送', exact: true })).toBeEnabled()
  await page.getByRole('textbox', { name: '消息内容', exact: true }).fill('长'.repeat(2000))
  await page.getByRole('button', { name: '发送', exact: true }).click()
  await page.getByRole('button', { name: '二维码', exact: true }).click()
  await expect(page.getByRole('alert')).toContainText('超过单个二维码的容量')
})

test('a supported original video plays in the native browser player', async ({ page }) => {
  await page.goto('/')
  const bytes = await page.evaluate(async () => {
    const canvas = document.createElement('canvas')
    canvas.width = 160; canvas.height = 90
    const context = canvas.getContext('2d')
    if (!context) throw new Error('Canvas unavailable')
    context.fillStyle = '#d96742'; context.fillRect(0, 0, 160, 90)
    const stream = canvas.captureStream(10)
    const recorder = new MediaRecorder(stream, { mimeType: 'video/webm;codecs=vp8' })
    const chunks: Blob[] = []
    recorder.ondataavailable = event => chunks.push(event.data)
    const done = new Promise<Blob>(resolve => { recorder.onstop = () => resolve(new Blob(chunks, { type: 'video/webm' })) })
    recorder.start()
    const animation = setInterval(() => { context.fillStyle = '#35362f'; context.fillRect(Math.random() * 150, 30, 10, 10) }, 50)
    await new Promise(resolve => setTimeout(resolve, 600))
    recorder.stop(); clearInterval(animation)
    const blob = await done
    for (const track of stream.getTracks()) track.stop()
    return Array.from(new Uint8Array(await blob.arrayBuffer()))
  })
  await page.getByLabel('选择文件').setInputFiles({ name: '原视频.webm', mimeType: 'video/webm', buffer: Buffer.from(bytes) })
  await expect(page.getByRole('button', { name: '发送', exact: true })).toBeEnabled()
  await page.getByRole('button', { name: '发送', exact: true }).click()
  await page.getByRole('button', { name: '预览 原视频.webm' }).click()
  const video = page.locator('video')
  await expect(video).toBeVisible()
  await expect.poll(() => video.evaluate(element => element instanceof HTMLVideoElement ? element.readyState : 0)).toBeGreaterThanOrEqual(2)
  await video.evaluate(async element => {
    if (element instanceof HTMLVideoElement) { element.muted = true; await element.play() }
  })
  await expect.poll(() => video.evaluate(element => element instanceof HTMLVideoElement ? element.currentTime : 0)).toBeGreaterThan(0)
})

test('numeric text uses QR numeric capacity instead of a byte-length cutoff', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('textbox', { name: '消息内容', exact: true }).fill('1234567890'.repeat(600))
  await page.getByRole('button', { name: '发送', exact: true }).click()
  await page.getByRole('button', { name: '二维码', exact: true }).click()
  await expect(page.getByRole('img', { name: '包含消息原文的二维码' })).toBeVisible()
  await expect(page.getByRole('dialog', { name: '消息二维码' }).getByRole('alert')).toHaveCount(0)
})

test('four parts run concurrently across files and a lost acknowledgement resumes safely', async ({ page }) => {
  test.setTimeout(90_000)
  const chunkSize = 8 * 1024 * 1024
  const original = Buffer.alloc(3 * chunkSize + 123, 37)
  const digest = createHash('sha256').update(original).digest('hex')
  let release: () => void = () => {}
  const gate = new Promise<void>(resolve => { release = resolve })
  let started = 0
  let active = 0
  let peak = 0
  let lostAck = false
  await page.route('**/api/uploads/*/chunks/*', async route => {
    started++
    peak = Math.max(peak, ++active)
    try {
      if (started <= 4) await gate
      const response = await route.fetch()
      if (!lostAck && route.request().url().endsWith(`/chunks/${chunkSize}`)) {
        lostAck = true
        expect(response.ok()).toBeTruthy()
        await route.abort('failed')
      } else await route.fulfill({ response })
    } finally { active-- }
  })
  await page.goto('/')
  await page.getByLabel('选择文件').setInputFiles([
    { name: '四路并发.bin', mimeType: 'application/octet-stream', buffer: original },
    { name: '同时上传.txt', mimeType: 'text/plain', buffer: Buffer.from('第二个文件') },
  ])
  try {
    await expect.poll(() => started).toBe(4)
    expect(await page.evaluate(async () => (await fetch('/api/health')).text())).toBe('ok')
    expect(started).toBe(4)
  } finally { release() }
  await expect(page.getByRole('button', { name: '发送', exact: true })).toBeEnabled({ timeout: 60_000 })
  expect(lostAck).toBe(true)
  expect(peak).toBe(4)
  await page.getByRole('button', { name: '发送', exact: true }).click()
  await page.getByRole('button', { name: '四路并发.bin 文件信息' }).click()
  await expect(page.locator('.file-details code')).toHaveText(digest)
  await expect(page.getByRole('button', { name: '预览 同时上传.txt' })).toBeVisible()
})
