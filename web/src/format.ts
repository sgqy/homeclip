export function formatSize(size: number): string {
  if (size < 1024) return `${size} B`
  const units = ['KiB', 'MiB', 'GiB', 'TiB', 'PiB', 'EiB']
  let value = size / 1024
  let index = 0
  while (value >= 1024 && index < units.length - 1) { value /= 1024; index++ }
  return `${value < 10 ? value.toFixed(1) : value.toFixed(0)} ${units[index] ?? 'EiB'}`
}

export function formatDate(timestamp: number): string {
  return new Intl.DateTimeFormat('zh-CN', {
    month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false,
  }).format(timestamp)
}

export async function copyText(text: string): Promise<boolean> {
  try { await navigator.clipboard.writeText(text); return true } catch { /* LAN HTTP fallback below. */ }
  const active = document.activeElement
  const field = document.createElement('textarea')
  field.value = text
  field.style.cssText = 'position:fixed;left:0;top:0;opacity:0;pointer-events:none'
  document.body.append(field)
  field.select()
  let copied = false
  try { copied = document.execCommand('copy') } catch { /* Show selectable text in the UI. */ }
  field.remove()
  if (active instanceof HTMLElement) active.focus()
  return copied
}
