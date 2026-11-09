/**
 * The workspace's custom emoji and stickers as the settings pages manage them: the calls, the server's rules for a new
 * one (the web's `lib/customEmoji.ts` and `lib/customSticker.ts`) and the server's refusals in words. No React.
 */
import { ApiProblem } from '@orbit/api/problem'
import { createChatEmoji, createChatSticker, deleteChatEmoji, deleteChatSticker, listChatStickers } from '@orbit/api/generated/sdk.gen'
import type { CustomEmojiRecord, CustomStickerRecord } from '@orbit/api/generated/types.gen'

import type { OrbitClient } from './orbit'

export type { CustomEmojiRecord, CustomStickerRecord }

/** A picked image before it uploads. `type` is `''` when the system does not know it (the server decides by the bytes). */
export interface PickedImage {
  uri: string
  name: string
  type: string
  size: number
}

export const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/webp', 'image/gif']

// ---- custom emoji (`chat/emoji` on the server)

const EMOJI_NAME = /^[a-z0-9_]{2,32}$/
export const EMOJI_MAX_BYTES = 256 * 1024
export const EMOJI_LIMIT = 200

/** The names a message shows as a character before any custom emoji (the web's `EMOJI_SHORTCODES`). */
export const BUILT_IN_SHORTCODES: ReadonlySet<string> = new Set(['white_check_mark', 'check', 'heavy_check_mark', 'x', 'cross_mark', 'crossmark', 'warning', 'information_source'])

/** A name to start from, made of a file name: `Party Parrot.gif` gives `party_parrot`. */
export function emojiNameFromFile(fileName: string): string {
  return fileName
    .replace(/\.[^.]*$/, '')
    .toLowerCase()
    .replace(/[^a-z0-9_]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 32)
}

/** Why a new emoji cannot be added, in words for the form; `null` when the server will take it. */
export function emojiProblem(file: { type: string; size: number }, name: string, taken: (name: string) => boolean): string | null {
  if (file.type !== '' && !IMAGE_TYPES.includes(file.type)) return 'Choose a PNG, JPEG, WebP or GIF image.'
  if (file.size > EMOJI_MAX_BYTES) return 'The image is too large. The limit is 256 KB.'
  if (!EMOJI_NAME.test(name)) return 'A name has 2 to 32 characters: lower case letters, digits and underscores.'
  if (taken(name)) return `:${name}: exists already. Choose another name.`
  return null
}

export const EMOJI_PROBLEMS: Record<string, string> = {
  chat_forbidden: 'Only workspace owners and administrators can manage custom emoji.',
  emoji_too_large: 'The image is too large. The limit is 256 KB.',
  invalid_emoji: 'The file is not a PNG, JPEG, WebP or GIF image.',
  validation_failed: 'A name has 2 to 32 characters: lower case letters, digits and underscores.',
  emoji_name_taken: 'An emoji with this name exists already. Choose another name.',
  emoji_limit_reached: 'The workspace has 200 custom emoji, which is the limit. Delete one first.',
  storage_quota_exceeded: 'The workspace storage is full. Delete files, or ask the administrator of this Orbit for a higher quota.',
}

// ---- stickers (`chat/stickers` on the server)

export const STICKER_MAX_BYTES = 512 * 1024
export const STICKER_NAME_MAX = 30
export const STICKER_LIMIT = 100

/** Two names are the same sticker name when only the case of their ASCII letters differs, as on the server. */
const foldCase = (name: string) => name.replace(/[A-Z]/g, (letter) => letter.toLowerCase())

/** A name to start from, made of a file name: `Party Parrot.gif` gives `Party Parrot`. */
export function stickerNameFromFile(fileName: string): string {
  const base = fileName.replace(/\.[^.]*$/, '').trim()
  return Array.from(base).slice(0, STICKER_NAME_MAX).join('').trim()
}

/** Why a new sticker cannot be added, in words for the form; `null` when the server will take it. The name is checked trimmed. */
export function stickerProblem(file: { type: string; size: number }, name: string, names: readonly string[]): string | null {
  const trimmed = name.trim()
  const length = Array.from(trimmed).length
  if (file.type !== '' && !IMAGE_TYPES.includes(file.type)) return 'Choose a PNG, JPEG, WebP or GIF image.'
  if (file.size > STICKER_MAX_BYTES) return 'The image is too large. The limit is 512 KB.'
  if (length < 2 || length > STICKER_NAME_MAX || /\p{Cc}/u.test(trimmed)) return 'A name has 2 to 30 characters.'
  if (names.some((used) => foldCase(used) === foldCase(trimmed))) return `A sticker named ${trimmed} exists already. Choose another name.`
  return null
}

export const STICKER_PROBLEMS: Record<string, string> = {
  chat_forbidden: 'Only workspace owners and administrators can manage stickers.',
  sticker_too_large: 'The image is too large. The limit is 512 KB.',
  storage_quota_exceeded: 'The workspace storage is full. Delete files, or ask the administrator of this Orbit for a higher quota.',
  invalid_sticker: 'The file is not a PNG, JPEG, WebP or GIF image.',
  validation_failed: 'A name has 2 to 30 characters.',
  sticker_name_taken: 'A sticker with this name exists already. Choose another name.',
  sticker_limit_reached: 'The workspace has 100 stickers, which is the limit. Delete one first.',
}

/** The reason in words: ours for a known code, else what the server said (with the status), else that it did not answer. */
export function serverProblem(error: unknown, fallback: string, problems: Record<string, string>): string {
  if (!(error instanceof ApiProblem)) return `${fallback} Orbit did not answer. Check your connection and try again.`
  return problems[error.code] ?? `${fallback} ${error.detail} (${error.status} ${error.code})`
}

// ---- calls

const at = (client: OrbitClient, workspaceId: string) => ({ client, path: { workspace_id: workspaceId }, throwOnError: true as const })

/** The generated serializer turns anything but a Blob into JSON; the app's `File` (`upload-part.ts`) only implements Blob. */
const filePart = (body: unknown) => {
  const form = new FormData()
  form.append('file', (body as { file: Blob }).file)
  return form
}

export async function createEmoji(client: OrbitClient, workspaceId: string, name: string, part: Blob): Promise<CustomEmojiRecord> {
  return (await createChatEmoji({ ...at(client, workspaceId), query: { name }, body: { file: part }, bodySerializer: filePart })).data
}

export async function deleteEmoji(client: OrbitClient, workspaceId: string, emojiId: string): Promise<void> {
  await deleteChatEmoji({ client, path: { workspace_id: workspaceId, emoji_id: emojiId }, throwOnError: true })
}

export async function stickers(client: OrbitClient, workspaceId: string): Promise<CustomStickerRecord[]> {
  return (await listChatStickers(at(client, workspaceId))).data
}

export async function createSticker(client: OrbitClient, workspaceId: string, name: string, part: Blob): Promise<CustomStickerRecord> {
  return (await createChatSticker({ ...at(client, workspaceId), query: { name }, body: { file: part }, bodySerializer: filePart })).data
}

export async function deleteSticker(client: OrbitClient, workspaceId: string, stickerId: string): Promise<void> {
  await deleteChatSticker({ client, path: { workspace_id: workspaceId, sticker_id: stickerId }, throwOnError: true })
}
