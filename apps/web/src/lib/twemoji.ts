import { parse } from '@twemoji/parser'

/**
 * Emoji show as Twemoji images, so they look the same on every device. The SVG files are served by Orbit itself
 * (`vite.config.ts` copies them from `@twemoji/svg`): the production CSP has no CDN, and a self-hosted Orbit must work
 * without one. The paths are under `/assets` and carry the package version, so the server sends them as immutable: a
 * browser downloads each file once and then never asks again. Graphics © Twitter, Inc. and other contributors, CC-BY 4.0.
 */
const TWEMOJI_PATH = '/assets/twemoji-15.0.0'

/** The emoji data that the picker loads, also served by Orbit (from `emojibase-data`). */
export const EMOJIBASE_PATH = '/assets/emojibase-17.0.0'

/**
 * The Unicode emoji version of the bundled images. The picker offers only emoji up to this version; a newer emoji
 * that someone types has no image and shows in the system font.
 */
export const EMOJI_VERSION = 15

export interface EmojiSegment {
  text: string
  /** The image for an emoji; `null` for ordinary text. */
  url: string | null
}

const buildUrl = (codepoints: string) => `${TWEMOJI_PATH}/${codepoints}.svg`
/** Text without a character above U+00A8 has no emoji: most text skips the emoji regex. */
const MAYBE_EMOJI = /[\u00a9-\uffff]/

/** Cuts text into ordinary text and emoji, in order. */
export function splitEmoji(text: string): EmojiSegment[] {
  if (!MAYBE_EMOJI.test(text)) return text ? [{ text, url: null }] : []
  const segments: EmojiSegment[] = []
  let cursor = 0
  for (const entity of parse(text, { buildUrl })) {
    const [start, end] = entity.indices
    if (!entity.url) continue
    if (start > cursor) segments.push({ text: text.slice(cursor, start), url: null })
    segments.push({ text: entity.text, url: entity.url })
    cursor = end
  }
  if (cursor < text.length) segments.push({ text: text.slice(cursor), url: null })
  return segments
}

/** The image of a value that is exactly one emoji (a reaction, a page icon); `null` for anything else. */
export function twemojiUrl(value: string): string | null {
  const segments = splitEmoji(value)
  return segments.length === 1 ? segments[0].url : null
}
