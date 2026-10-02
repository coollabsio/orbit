import { splitEmoji } from '@/lib/twemoji'
import { findMentions, type MentionTarget } from '../../lib/mentionTokens'

/** A piece of the text in the message box, as the box shows it. The pieces join back to the exact text. */
export type ComposerSegment =
  | { kind: 'text'; text: string }
  /** `@Name`, `#name`, `@channel`, `@here`: the same matches that are sent as mentions. */
  | { kind: 'mention'; text: string }
  | { kind: 'emoji'; text: string; url: string }

export function composerSegments(text: string, people: readonly MentionTarget[], conversations: readonly MentionTarget[]): ComposerSegment[] {
  const segments: ComposerSegment[] = []
  const plain = (part: string) => {
    for (const piece of splitEmoji(part)) segments.push(piece.url ? { kind: 'emoji', text: piece.text, url: piece.url } : { kind: 'text', text: piece.text })
  }
  let cursor = 0
  for (const mention of findMentions(text, people, conversations)) {
    plain(text.slice(cursor, mention.start))
    segments.push({ kind: 'mention', text: text.slice(mention.start, mention.end) })
    cursor = mention.end
  }
  plain(text.slice(cursor))
  return segments
}
