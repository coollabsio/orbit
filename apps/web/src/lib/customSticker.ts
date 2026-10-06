/** The server's rules (`chat/stickers`): the image types, the size and the length of a name. */
export const STICKER_IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/webp', 'image/gif']
export const STICKER_MAX_BYTES = 512 * 1024
export const STICKER_NAME_MAX = 30

/** Two names are the same sticker name when only the case of their ASCII letters differs, as on the server. */
const foldCase = (name: string) => name.replace(/[A-Z]/g, (letter) => letter.toLowerCase())

/** A name to start from, made of a file name: `Party Parrot.gif` gives `Party Parrot`. */
export function stickerNameFromFile(fileName: string): string {
  const base = fileName.replace(/\.[^.]*$/, '').trim()
  return Array.from(base).slice(0, STICKER_NAME_MAX).join('').trim()
}

/** The stickers whose name contains the search text, without regard to case. An empty search keeps all. */
export function searchStickers<T extends { name: string }>(stickers: readonly T[], search: string): T[] {
  const query = search.trim().toLowerCase()
  return stickers.filter((sticker) => sticker.name.toLowerCase().includes(query))
}

/**
 * Why a new sticker cannot be added, in words for the form; `null` when the server will take it. `names` are the
 * names in use. The name is checked as it is sent: trimmed.
 */
export function stickerProblem(file: { type: string; size: number }, name: string, names: readonly string[]): string | null {
  const trimmed = name.trim()
  const length = Array.from(trimmed).length
  // a system that does not know the type sends none: the server decides by the bytes
  if (file.type !== '' && !STICKER_IMAGE_TYPES.includes(file.type)) return 'Choose a PNG, JPEG, WebP or GIF image.'
  if (file.size > STICKER_MAX_BYTES) return 'The image is too large. The limit is 512 KB.'
  if (length < 2 || length > STICKER_NAME_MAX || /\p{Cc}/u.test(trimmed)) return 'A name has 2 to 30 characters.'
  if (names.some((used) => foldCase(used) === foldCase(trimmed))) return `A sticker named ${trimmed} exists already. Choose another name.`
  return null
}
