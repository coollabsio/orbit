import type { CellValue, FileRef, Property, UniqueIdConfig } from './types'

/** Spec P3 item 10 limits, as the server's. */
export const MAX_FILES = 20
export const MAX_EMAIL_CHARS = 320
export const MAX_PHONE_CHARS = 50
const MAX_FILE_NAME_CHARS = 255
const MAX_PREFIX_CHARS = 10

/** One `@` with text on both sides, no whitespace. */
export function validEmail(text: string): boolean {
  if (Array.from(text).length > MAX_EMAIL_CHARS || /\s/u.test(text)) return false
  const at = text.indexOf('@')
  return at > 0 && at < text.length - 1 && text.indexOf('@', at + 1) === -1
}

/** Digits, spaces and `+ - ( ) .`, with at least one digit. */
export function validPhone(text: string): boolean {
  return Array.from(text).length <= MAX_PHONE_CHARS && /\d/.test(text) && /^[\d +\-().]+$/.test(text)
}

export function validPrefix(prefix: string): boolean {
  return prefix.length >= 1 && prefix.length <= MAX_PREFIX_CHARS && /^[A-Za-z0-9_]+$/.test(prefix)
}

export function parseUniqueIdConfig(raw: Record<string, unknown>): UniqueIdConfig {
  const prefix = typeof raw.prefix === 'string' && validPrefix(raw.prefix) ? raw.prefix : null
  const next = typeof raw.next === 'number' && Number.isInteger(raw.next) && raw.next >= 1 ? raw.next : 1
  return { prefix, next }
}

/** A stored files value: unique file ids with names, at most 20; `null` when empty or malformed. */
export function parseFiles(raw: unknown): CellValue | null {
  if (!Array.isArray(raw) || raw.length === 0 || raw.length > MAX_FILES) return null
  const files: FileRef[] = []
  const seen = new Set<string>()
  for (const item of raw as unknown[]) {
    if (typeof item !== 'object' || item === null) return null
    const { file_id, name } = item as Record<string, unknown>
    if (typeof file_id !== 'string' || typeof name !== 'string' || name === '') return null
    if (Array.from(name).length > MAX_FILE_NAME_CHARS || seen.has(file_id)) return null
    seen.add(file_id)
    files.push({ file_id, name })
  }
  return { kind: 'files', files }
}

/** `PREFIX-12`, or `12` without a prefix. */
export function uniqueIdText(property: Property, number: number): string {
  const prefix = property.type === 'unique_id' ? property.config.prefix : null
  return prefix ? `${prefix}-${number}` : String(number)
}

/** The `mailto:` or `tel:` link of an email or phone value. */
export function contactHref(type: 'email' | 'phone', text: string): string {
  return type === 'email' ? `mailto:${text}` : `tel:${text.replace(/[^\d+]/g, '')}`
}

/** The same-origin download path of a file of a row (the page file route). */
export function rowFileUrl(workspaceId: string, rowId: string, fileId: string): string {
  return `/api/v1/workspaces/${workspaceId}/pages/${rowId}/files/${fileId}`
}

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'avif'])

/** Whether a file name names an image type the server shows inline. */
export function isImageName(name: string): boolean {
  const dot = name.lastIndexOf('.')
  return dot !== -1 && IMAGE_EXTENSIONS.has(name.slice(dot + 1).toLowerCase())
}

/** The first image of a files value, by file name (for gallery card covers); `null` without one. */
export function firstImageFile(value: CellValue | null | undefined): FileRef | null {
  return value?.kind === 'files' ? (value.files.find((file) => isImageName(file.name)) ?? null) : null
}
