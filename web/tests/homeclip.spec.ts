import { createHash, randomUUID } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import { expect, test } from '@playwright/test'
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
