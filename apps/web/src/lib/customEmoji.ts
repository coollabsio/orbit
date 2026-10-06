/** What the app needs of a workspace's custom emoji to show it. `url` is the image, served by Orbit. */
export interface CustomEmoji {
  /** Without colons: `party_parrot`. */
  name: string
  url: string
}

/** The server's rules (`chat/emoji`): the name, the image types and the size. */
const NAME = /^[a-z0-9_]{2,32}$/
export const EMOJI_IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/webp', 'image/gif']
export const EMOJI_MAX_BYTES = 256 * 1024

/** The name in a value that is exactly `:name:` (a reaction, a token in a message); `null` for anything else. */
export function customEmojiName(value: string): string | null {
  return value.startsWith(':') && value.endsWith(':') && NAME.test(value.slice(1, -1)) ? value.slice(1, -1) : null
}

/** A name to start from, made of a file name: `Party Parrot.gif` gives `party_parrot`. */
export function emojiNameFromFile(fileName: string): string {
  return fileName
    .replace(/\.[^.]*$/, '')
    .toLowerCase()
    .replace(/[^a-z0-9_]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 32)
}

/**
 * Why a new emoji cannot be added, in words for the form; `null` when the server will take it. `taken` says whether
 * a name is in use already, by a custom emoji or a standard one (`:joy:`).
 */
export function emojiProblem(file: { type: string; size: number }, name: string, taken: (name: string) => boolean): string | null {
  if (file.type !== '' && !EMOJI_IMAGE_TYPES.includes(file.type)) return 'Choose a PNG, JPEG, WebP or GIF image.'
  if (file.size > EMOJI_MAX_BYTES) return 'The image is too large. The limit is 256 KB.'
  if (!NAME.test(name)) return 'A name has 2 to 32 characters: lower case letters, digits and underscores.'
  if (taken(name)) return `:${name}: exists already. Choose another name.`
  return null
}
