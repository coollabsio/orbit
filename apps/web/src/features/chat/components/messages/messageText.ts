import { splitEmoji } from '@/lib/twemoji'
import type { Message } from '../../api/types'

/** A message longer than this many lines collapses behind "Show more". */
export const COLLAPSE_AFTER_LINES = 20
/** The text measure: a source line longer than this wraps. */
const MEASURE = 90
const DEFAULT_QUICK_REACTIONS = ['👍', '👀', '😂']

/** Fenced and inline code: a URL or a mention inside it is text. */
const CODE = /```[\s\S]*?```|`[^`\n]+`/g
const URL = /https?:\/\/[^\s<>]+/g
/** A URL in angle brackets (`<https://…>`) is a link without a card, as in Discord. After a backslash the bracket is text. */
const WRAPPED_URL = /(?<!\\)<https?:\/\/[^\s<>]+>/g
/** A message of only emoji shows them large up to this many. */
const JUMBO_EMOJI_LIMIT = 30

/** The part of a stored body whose URLs can become link cards: without code and URLs in angle brackets. */
function cardText(body: string): string {
  return body.replace(CODE, ' ').replace(WRAPPED_URL, ' ')
}

/**
 * Lines a message takes at the full text measure: each source line counts once for every 90 characters. An estimate
 * (the real wrap depends on the column width), which is enough to decide whether to collapse.
 */
export function displayLineCount(text: string): number {
  if (text === '') return 0
  return text.split('\n').reduce((lines, line) => lines + Math.max(1, Math.ceil(line.length / MEASURE)), 0)
}

export function isLongMessage(text: string): boolean {
  return displayLineCount(text) > COLLAPSE_AFTER_LINES
}

/**
 * URLs in a stored body that can become link cards: outside code and angle brackets, without trailing punctuation, each
 * one once.
 */
export function extractLinkUrls(body: string, limit = 3): string[] {
  const urls: string[] = []
  for (const match of cardText(body).matchAll(URL)) {
    const url = match[0].replace(/[.,!?;:)}\]'"]+$/, '')
    if (url && !urls.includes(url)) urls.push(url)
    if (urls.length === limit) break
  }
  return urls
}

/**
 * Of the URLs that can become link cards, the ones that are a whole line of the body and are nowhere else in it. The
 * card stands in for such a URL, so the text can leave it out; a URL inside a sentence stays where it is.
 */
export function standaloneLinkUrls(body: string): string[] {
  const text = cardText(body)
  const lines = new Set(text.split('\n'))
  return extractLinkUrls(body).filter((url) => lines.has(url) && text.split(url).length === 2)
}

/**
 * Whether a shown text (mentions decoded) is only emoji and white space, with 1 to 30 emoji: such a message shows them
 * large, as in Discord. A flag, a skin tone or a ZWJ sequence is one emoji. `:name:` counts when `customNames` has the
 * name; an unknown or escaped (`\:name:`) one is text.
 */
export function isJumboEmoji(text: string, customNames: Pick<ReadonlySet<string>, 'has'>): boolean {
  let count = 0
  const rest = text.replace(/(?<!\\):([a-z0-9_+-]+):/g, (token, name: string) => {
    if (!customNames.has(name)) return token
    count += 1
    return ' '
  })
  for (const segment of splitEmoji(rest)) {
    if (segment.url) count += 1
    else if (segment.text.trim() !== '') return false
  }
  return count >= 1 && count <= JUMBO_EMOJI_LIMIT
}

/** The first line with text, cut to `max` characters: the thread breadcrumb and announcements. */
export function firstLine(text: string, max = 80): string {
  const line = text.split('\n').find((candidate) => candidate.trim() !== '')?.trim() ?? ''
  return line.length > max ? `${line.slice(0, max - 1).trimEnd()}…` : line
}

/**
 * The three reactions of the hover toolbar: the ones the user used most in the loaded messages, then the defaults.
 */
export function quickReactions(messages: readonly Message[], userId: string | null): string[] {
  const counts = new Map<string, number>()
  if (userId) {
    for (const message of messages) {
      for (const reaction of message.reactions) {
        if (reaction.userIds.includes(userId)) counts.set(reaction.emoji, (counts.get(reaction.emoji) ?? 0) + 1)
      }
    }
  }
  const used = [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([emoji]) => emoji)
  return [...new Set([...used, ...DEFAULT_QUICK_REACTIONS])].slice(0, 3)
}

/** "Ada is typing", "Ada and Grace are typing", then "Several people are typing". Empty when nobody types. */
export function typingLabel(names: readonly string[]): string {
  if (names.length === 0) return ''
  if (names.length === 1) return `${names[0]} is typing`
  if (names.length === 2) return `${names[0]} and ${names[1]} are typing`
  return 'Several people are typing'
}

/** "1 reply", "4 replies". */
export function replyCountLabel(count: number): string {
  return count === 1 ? '1 reply' : `${count} replies`
}

/** Messages of other members that came after the message with `lastKey`; none when that message is gone. */
export function messagesAfter(
  messages: readonly Message[],
  lastKey: string | null,
  keyOf: (message: Message) => string,
  currentUserId: string | null,
): Message[] {
  if (lastKey === null) return []
  const index = messages.findIndex((message) => keyOf(message) === lastKey)
  if (index === -1) return []
  return messages.slice(index + 1).filter((message) => message.kind === 'message' && message.authorId !== currentUserId)
}
