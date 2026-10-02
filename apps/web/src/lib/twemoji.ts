import { parse } from '@twemoji/parser'
import { version as emojibaseVersion } from 'emojibase-data/package.json'
import { version as twemojiVersion } from 'twemoji-assets/package.json'

/**
 * Emoji show as Twemoji images, so they look the same on every device. The SVG files are served by Orbit itself
 * (`vite.config.ts` copies them from `twemoji-assets`): the production CSP has no CDN, and a self-hosted Orbit must work
 * without one. The paths are under `/assets` and carry the package version, so the server sends them as immutable: a
 * browser downloads each file once and then never asks again. Graphics © Twitter, Inc. and other contributors, CC-BY 4.0.
 */
const TWEMOJI_PATH = `/assets/twemoji-${twemojiVersion}`

/** The emoji data that the picker loads, also served by Orbit (from `emojibase-data`). */
export const EMOJIBASE_PATH = `/assets/emojibase-${emojibaseVersion}`

/**
 * The Unicode emoji version of the bundled images. The picker offers only emoji up to this version; a newer emoji
 * that someone types has no image and shows in the system font.
 */
export const EMOJI_VERSION = 17

export interface EmojiSegment {
  text: string
  /** The image for an emoji; `null` for ordinary text. */
  url: string | null
}

/** The name of the image file. Only the eye in a speech bubble has a file without its variation selectors. */
const buildUrl = (codepoints: string) =>
  `${TWEMOJI_PATH}/${codepoints === '1f441-fe0f-200d-1f5e8-fe0f' ? '1f441-200d-1f5e8' : codepoints}.svg`
/** Text without a character above U+00A8 has no emoji: most text skips the emoji regex. */
const MAYBE_EMOJI = /[\u00a9-\uffff]/
const VARIATION_SELECTOR = '\ufe0f'
const SKIN_TONE = /^\ud83c[\udffb-\udfff]$/
/**
 * Ten emoji that the parser does not find alone (☝ ⛷ ⛹ ✌ ✍ 🏋 🏌 🕴 🕵 🖐), with a selector or a skin tone after
 * them. Their images exist.
 */
const MISSED = /([\u261d\u26f7\u26f9\u270c\u270d]|\ud83c[\udfcb\udfcc]|\ud83d[\udd74\udd75\udd90])\ufe0f?(\ud83c[\udffb-\udfff])?/g
const hex = (character: string) => character.codePointAt(0)!.toString(16)

/** Text between the emoji that the parser found: ordinary text, and the emoji that it missed. */
function pushText(segments: EmojiSegment[], text: string) {
  let cursor = 0
  for (const match of text.matchAll(MISSED)) {
    if (match.index > cursor) segments.push({ text: text.slice(cursor, match.index), url: null })
    segments.push({ text: match[0], url: buildUrl(match[2] ? `${hex(match[1])}-${hex(match[2])}` : hex(match[1])) })
    cursor = match.index + match[0].length
  }
  if (cursor < text.length) segments.push({ text: text.slice(cursor), url: null })
}

/** Cuts text into ordinary text and emoji, in order. */
export function splitEmoji(text: string): EmojiSegment[] {
  if (!MAYBE_EMOJI.test(text)) return text ? [{ text, url: null }] : []
  const segments: EmojiSegment[] = []
  let cursor = 0
  for (const entity of parse(text, { buildUrl })) {
    const [start, end] = entity.indices
    if (!entity.url) {
      // A selector that the parser did not take with its emoji ("👍" + U+FE0F) is part of that emoji.
      const last = segments.at(-1)
      if (entity.text === VARIATION_SELECTOR && start === cursor && last?.url) {
        last.text += entity.text
        cursor = end
      }
      continue
    }
    // A skin tone directly after other text is not an emoji alone: it belongs to a missed emoji, or it is text.
    if (SKIN_TONE.test(entity.text) && start > cursor && !/\s/.test(text[start - 1])) continue
    if (start > cursor) pushText(segments, text.slice(cursor, start))
    segments.push({ text: entity.text, url: entity.url })
    cursor = end
  }
  if (cursor < text.length) pushText(segments, text.slice(cursor))
  return segments
}

/** An emoji from the picker in the form that a keyboard types: without a selector that adds nothing ("👍" + U+FE0F). */
export function normalizeEmoji(value: string): string {
  const entities = parse(value, { buildUrl })
  return entities.length === 2 && entities[1].text === VARIATION_SELECTOR && !entities[1].url ? entities[0].text : value
}

/** The image of a value that is exactly one emoji (a reaction, a page icon); `null` for anything else. */
export function twemojiUrl(value: string): string | null {
  const segments = splitEmoji(value)
  return segments.length === 1 ? segments[0].url : null
}
