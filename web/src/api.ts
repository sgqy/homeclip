import { z } from 'zod'

export const attachmentSchema = z.object({
  id: z.string(), name: z.string(), size: z.number().nonnegative(), mime: z.string(), sha256: z.string(),
})
export type Attachment = z.infer<typeof attachmentSchema>

export const messageSchema = z.object({
  id: z.number().int(), client_id: z.string(), text: z.string(),
  created_at: z.number(), updated_at: z.number(),
  reply_to: z.number().nullable(), reply_excerpt: z.string().nullable(),
  attachments: z.array(attachmentSchema),
})
export type Message = z.infer<typeof messageSchema>

export const pageSchema = z.object({
  messages: z.array(messageSchema), next_before: z.number().nullable(), cursor: z.number(),
})
export const changeSchema = z.object({
  id: z.number(), kind: z.enum(['upsert', 'delete']), message_id: z.number(),
})
export const uploadSchema = attachmentSchema.omit({ sha256: true }).extend({
  received: z.number(), sha256: z.string().nullable(),
})
export const configSchema = z.object({ chunk_size: z.number().positive(), version: z.string() })

export class ApiError extends Error {
  constructor(message: string, readonly status: number) { super(message) }
}

export async function request(path: string, options: RequestInit = {}): Promise<Response> {
  const headers = new Headers(options.headers)
  if (typeof options.body === 'string') headers.set('content-type', 'application/json')
  const response = await fetch(`/api${path}`, { ...options, headers })
  if (!response.ok) {
    const body: unknown = await response.json().catch(() => null)
    const parsed = z.object({ error: z.string() }).safeParse(body)
    throw new ApiError(parsed.success ? parsed.data.error : `请求失败（${response.status}）`, response.status)
  }
  return response
}

export async function api<T>(path: string, schema: z.ZodType<T>, options?: RequestInit): Promise<T> {
  const response = await request(path, options)
  const body: unknown = await response.json()
  return schema.parse(body)
}

export function uniqueId(): string {
  // getRandomValues is also available on ordinary LAN HTTP origins.
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), b => b.toString(16).padStart(2, '0')).join('')
}

export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : '操作失败，请重试'
}

export function fileUrl(file: Attachment, download = false): string {
  return `/api/files/${encodeURIComponent(file.id)}/content${download ? '?download=1' : ''}`
}
