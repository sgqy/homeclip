import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createHash, randomUUID } from 'node:crypto'
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'

const image = process.argv[2] ?? 'homeclip:latest'
const container = `homeclip-check-${randomUUID()}`
const temporaryRoot = realpathSync(tmpdir())
const directory = mkdtempSync(path.join(temporaryRoot, 'homeclip-check-'))
const fileSize = 256 * 1024 * 1024
const chunkSize = 8 * 1024 * 1024
const requestedConcurrency = process.argv[3] === undefined ? undefined : Number(process.argv[3])
if (requestedConcurrency !== undefined) assert.ok(Number.isInteger(requestedConcurrency) && requestedConcurrency > 0 && requestedConcurrency <= 8)
let created = false
let monitor
let peakRss = 0
let base

function docker(...args) {
  return execFileSync('docker', args, { encoding: 'utf8', windowsHide: true, timeout: 45_000, stdio: ['ignore', 'pipe', 'pipe'] }).trim()
}
function sampleRss() {
  try {
    const lines = docker('top', container, '-eo', 'pid,rss,args').split('\n').slice(1)
    const rss = lines.reduce((sum, line) => sum + Number(line.trim().split(/\s+/)[1] ?? 0), 0) / 1024
    peakRss = Math.max(peakRss, rss)
    return rss
  } catch { return 0 }
}
async function health() {
  const port = docker('inspect', '--format', '{{(index (index .NetworkSettings.Ports "8080/tcp") 0).HostPort}}', container)
  base = `http://127.0.0.1:${port}`
  for (let attempt = 0; attempt < 60; attempt++) {
    try { if ((await fetch(`${base}/api/health`)).ok) return } catch { /* Starting. */ }
    await delay(500)
  }
  throw new Error('Container did not become ready')
}
async function json(url, options = {}) {
  const response = await fetch(`${base}/api${url}`, options)
  assert.equal(response.status, 200, await response.clone().text())
  return response.json()
}
function post(data) { return { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(data) } }

try {
  // Bind-mounted test files must remain removable by the host's unprivileged runner.
  const user = typeof process.getuid === 'function' && typeof process.getgid === 'function'
    ? ['--user', `${process.getuid()}:${process.getgid()}`]
    : []
  docker('run', '-d', '--name', container,
    ...user, '-p', '127.0.0.1::8080', '--mount', `type=bind,source=${directory},target=/data`, image)
  created = true
  await health()
  docker('exec', container, '/homeclip', '--healthcheck')
  const idleRss = sampleRss()
  monitor = setInterval(sampleRss, 1000)
  const seeded = await json('/messages', post({ client_id: randomUUID(), text: '持久化检查' }))
  const config = await json('/config')
  assert.equal(config.chunk_size, chunkSize)
  const concurrency = requestedConcurrency ?? config.upload_concurrency ?? 1
  const parallel = config.upload_concurrency !== undefined
  if (!parallel) assert.equal(concurrency, 1, 'The old protocol only supports sequential chunks')
  const resumedBytes = parallel ? Buffer.concat([Buffer.alloc(chunkSize, 7), Buffer.from('end')]) : Buffer.from('abcdef')
  const resume = await json('/uploads', post({ name: 'restart.bin', size: resumedBytes.length, sha256: createHash('sha256').update(resumedBytes).digest('hex') }))
  const saved = parallel
    ? await json(`/uploads/${resume.id}/chunks/${chunkSize}`, { method: 'PUT', body: Buffer.from('end') })
    : await json(`/uploads/${resume.id}`, { method: 'PATCH', headers: { 'upload-offset': '0' }, body: Buffer.from('abc') })
  assert.equal(saved.received, parallel ? 0 : 3)
  docker('kill', container)
  docker('start', container)
  await health()
  const progress = await json(`/uploads/${resume.id}`)
  assert.equal(progress.received, parallel ? 0 : 3)
  if (parallel) assert.equal(progress.uploaded, 3, 'Out-of-order committed ranges must survive restart')
  assert.equal((await json(`/messages/${seeded.id}`)).text, '持久化检查')
  if (parallel) await json(`/uploads/${resume.id}/chunks/0`, { method: 'PUT', body: resumedBytes.subarray(0, chunkSize) })
  else await json(`/uploads/${resume.id}`, { method: 'PATCH', headers: { 'upload-offset': '3' }, body: Buffer.from('def') })
  assert.equal((await json(`/uploads/${resume.id}/complete`, { method: 'POST' })).sha256, createHash('sha256').update(resumedBytes).digest('hex'))
  await fetch(`${base}/api/uploads/${resume.id}`, { method: 'DELETE' })

  const expectedHash = createHash('sha256')
  // Prepare the expected digest outside the upload timer.
  const hashBuffer = Buffer.alloc(chunkSize)
  for (let index = 0; index < fileSize / chunkSize; index++) expectedHash.update(hashBuffer.fill(index % 251))
  const digest = expectedHash.digest('hex')
  const file = await json('/uploads', post({ name: 'large-original.bin', size: fileSize, sha256: digest }))
  const started = performance.now()
  for (let offset = 0; offset < fileSize;) {
    const requests = []
    for (let index = 0; index < concurrency && offset < fileSize; index++, offset += chunkSize) {
      const buffer = Buffer.alloc(Math.min(chunkSize, fileSize - offset), (offset / chunkSize) % 251)
      requests.push(parallel
        ? json(`/uploads/${file.id}/chunks/${offset}`, { method: 'PUT', body: buffer })
        : json(`/uploads/${file.id}`, { method: 'PATCH', headers: { 'upload-offset': String(offset) }, body: buffer }))
    }
    await Promise.all(requests)
  }
  const uploadSeconds = (performance.now() - started) / 1000
  assert.equal((await json(`/uploads/${file.id}`)).received, fileSize)
  const hashStarted = performance.now()
  const complete = await json(`/uploads/${file.id}/complete`, { method: 'POST' })
  const hashSeconds = (performance.now() - hashStarted) / 1000
  assert.equal(complete.sha256, digest)
  const firstMessage = await json('/messages', post({ client_id: randomUUID(), text: '原文件检查', attachments: [file.id] }))
  const duplicate = await json('/uploads', post({ name: 'same-content.bin', size: fileSize, sha256: digest }))
  assert.equal(duplicate.sha256, digest, 'Matching bytes must skip upload')
  assert.equal(duplicate.received, fileSize)
  const secondMessage = await json('/messages', post({ client_id: randomUUID(), attachments: [file.id, duplicate.id] }))
  assert.equal(secondMessage.attachments.length, 2)
  assert.equal((await fetch(`${base}/api/messages/${firstMessage.id}`, { method: 'DELETE' })).status, 204)
  const range = await fetch(`${base}/api/files/${file.id}/content`, { headers: { Range: 'bytes=-257' } })
  assert.equal(range.status, 206)
  assert.deepEqual(Buffer.from(await range.arrayBuffer()), Buffer.alloc(257, 31))
  const download = await fetch(`${base}/api/files/${file.id}/content?download=1`)
  assert.equal(download.status, 200)
  const actualHash = createHash('sha256')
  let received = 0
  for await (const chunk of download.body) { actualHash.update(chunk); received += chunk.length }
  assert.equal(received, fileSize)
  assert.equal(actualHash.digest('hex'), digest)
  sampleRss()
  const events = await fetch(`${base}/api/events?after=0`)
  const reader = events.body.getReader()
  await reader.read()
  clearInterval(monitor)
  const stopStarted = performance.now()
  docker('stop', '--time', '10', container)
  const stopSeconds = (performance.now() - stopStarted) / 1000
  assert.equal(docker('inspect', '--format', '{{.State.ExitCode}}', container), '0', 'SIGTERM must close SSE and exit normally')
  await reader.cancel().catch(() => {})
  const metadata = JSON.parse(docker('image', 'inspect', image))[0]
  assert.equal(metadata.Architecture, 'amd64')
  const result = {
    image, architecture: metadata.Architecture, image_mib: metadata.Size / 1024 / 1024,
    idle_rss_mib: idleRss, peak_sampled_rss_mib: peakRss,
    transport: 'host loopback to Docker port, host bind-mounted storage', upload_concurrency: concurrency,
    original_file_mib: fileSize / 1024 / 1024, sha256: digest, upload_seconds: uploadSeconds, hash_seconds: hashSeconds,
    upload_mib_per_second: fileSize / 1024 / 1024 / uploadSeconds, upload_mbps: fileSize * 8 / 1_000_000 / uploadSeconds,
    sigterm_exit_seconds: stopSeconds, checks: ['healthcheck', 'forced restart recovery', 'message persistence', '256 MiB upload', 'client/server SHA-256', 'instant upload', 'shared attachments after deletion', 'suffix Range', 'original download', 'shutdown with active SSE'],
  }
  mkdirSync('.local', { recursive: true })
  writeFileSync('.local/image-check.json', `${JSON.stringify(result, null, 2)}\n`)
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`)
} catch (error) {
  if (created) process.stderr.write(`${docker('logs', container)}\n`)
  throw error
} finally {
  clearInterval(monitor)
  if (created) docker('rm', '-f', container)
  const resolved = realpathSync(directory)
  assert.equal(path.dirname(resolved), temporaryRoot)
  assert.ok(path.basename(resolved).startsWith('homeclip-check-'))
  rmSync(resolved, { recursive: true, force: true })
}
