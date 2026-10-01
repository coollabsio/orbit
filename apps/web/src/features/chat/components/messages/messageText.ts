import type { Message } from '../../api/types'

/** A message longer than this many lines collapses behind "Show more". */
export const COLLAPSE_AFTER_LINES = 20
/** The text measure: a source line longer than this wraps. */
const MEASURE = 90
const DEFAULT_QUICK_REACTIONS = ['👍', '👀', '😂']

/** Fenced and inline code: a URL or a mention inside it is text. */
const CODE = /```[\s\S]*?```|`[^`\n]+`/g
const URL = /https?:\/\/[^\s<>]+/g

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

/** URLs in a stored body that can become link cards: outside code, without trailing punctuation, each one once. */
export function extractLinkUrls(body: string, limit = 3): string[] {
  const urls: string[] = []
  for (const match of body.replace(CODE, ' ').matchAll(URL)) {
    const url = match[0].replace(/[.,!?;:)}\]'"]+$/, '')
    if (url && !urls.includes(url)) urls.push(url)
    if (urls.length === limit) break
  }
  return urls
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
