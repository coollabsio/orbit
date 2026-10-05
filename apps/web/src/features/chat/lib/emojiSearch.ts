import { EMOJIBASE_PATH, EMOJI_VERSION } from '@/lib/twemoji'

/** An emoji that `:name` can find. */
export interface EmojiShortcode {
  emoji: string
  /** The name to type, without colons: `joy`. */
  name: string
}

interface EmojibaseEmoji {
  hexcode: string
  emoji: string
  version: number
}

/** One entry for each shortcode, in the data's order (the common emoji first). Emoji newer than the images are left out. */
export function buildEmojiIndex(emojis: readonly EmojibaseEmoji[], shortcodes: Readonly<Record<string, string | string[]>>): EmojiShortcode[] {
  return emojis.flatMap((entry) => {
    if (entry.version > EMOJI_VERSION) return []
    const names = shortcodes[entry.hexcode]
    return (Array.isArray(names) ? names : names ? [names] : []).map((name) => ({ emoji: entry.emoji, name }))
  })
}

/** Names that start with the query, then names that contain it; one result for each emoji. */
export function searchEmojiIndex(index: readonly EmojiShortcode[], query: string, limit = 8): EmojiShortcode[] {
  const text = query.toLowerCase()
  const seen = new Set<string>()
  const results: EmojiShortcode[] = []
  for (const starts of [true, false]) {
    for (const entry of index) {
      if (results.length >= limit) return results
      if (seen.has(entry.emoji)) continue
      if (starts ? entry.name.startsWith(text) : entry.name.includes(text)) {
        seen.add(entry.emoji)
        results.push(entry)
      }
    }
  }
  return results
}

/**
 * The `:name` list: the workspace's custom emoji that match first, then the standard ones. A custom emoji is an entry
 * whose `emoji` is the text `:name:`, which is what a message keeps of it.
 */
export function suggestEmoji(custom: Iterable<string>, index: readonly EmojiShortcode[], query: string, limit = 8): EmojiShortcode[] {
  const first = searchEmojiIndex(Array.from(custom, (name) => ({ emoji: `:${name}:`, name })), query, limit)
  return [...first, ...searchEmojiIndex(index, query, limit - first.length)]
}

/** What `:name:` typed in full becomes: the standard emoji, or `null` to leave the text (a custom emoji stays `:name:`). */
export function completedEmoji(custom: { has(name: string): boolean }, index: readonly EmojiShortcode[], name: string): string | null {
  return custom.has(name) ? null : emojiNamed(index, name)
}

/** The emoji whose name is exactly this, for `:joy:` typed in full. */
export function emojiNamed(index: readonly EmojiShortcode[], name: string): string | null {
  const text = name.toLowerCase()
  return index.find((entry) => entry.name === text)?.emoji ?? null
}

let loading: Promise<EmojiShortcode[]> | null = null

/** Loads the index once, from Orbit's own copy of the emoji data. A failed load can be tried again. */
export function loadEmojiIndex(): Promise<EmojiShortcode[]> {
  loading ??= Promise.all([
    fetch(`${EMOJIBASE_PATH}/en/data.json`).then((response) => response.json() as Promise<EmojibaseEmoji[]>),
    fetch(`${EMOJIBASE_PATH}/en/shortcodes/github.json`).then((response) => response.json() as Promise<Record<string, string | string[]>>),
  ])
    .then(([emojis, shortcodes]) => buildEmojiIndex(emojis, shortcodes))
    .catch((error: unknown) => {
      loading = null
      throw error
    })
  return loading
}
