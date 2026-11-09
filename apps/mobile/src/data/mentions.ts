/**
 * `@` and `#` mentions in a plain text field, as the web composer does them (`useMentionAutocomplete` and
 * `mentionTokens` there): where the open mention is, what it suggests, the text after a pick, and the typed `@Name`
 * matches that are sent as mentions.
 */

/** Someone or something a mention can name. `handle` (the email's local part) also matches a query. */
export interface MentionTarget {
  id: string
  name: string
  handle?: string
}

export type MentionTrigger = '@' | '#'

/** The mention being typed: `[start, end)` holds the trigger and the query, and `end` is the cursor. */
export interface MentionQuery {
  start: number
  end: number
  query: string
  trigger: MentionTrigger
}

export interface MentionSuggestion {
  id: string
  label: string
  /** The handle of a person; a note such as "everyone here" for `@channel`. */
  detail: string
  kind: 'user' | 'channel'
}

/** `@channel` and `@here`, offered in a conversation (not in a thread). */
export const GLOBAL_MENTIONS: readonly MentionTarget[] = [
  { id: '!channel', name: 'channel', handle: 'everyone here' },
  { id: '!here', name: 'here', handle: 'everyone online' },
]

/**
 * The mention the cursor is in: the last `@` (or `#` when `channels` is set) before it, at the start or after white
 * space, with no new line and at most 48 characters after it. A name can hold spaces, so the query can too.
 */
export function mentionQuery(text: string, cursor: number, channels = false): MentionQuery | null {
  const before = text.slice(0, Math.min(cursor, text.length))
  const start = Math.max(before.lastIndexOf('@'), channels ? before.lastIndexOf('#') : -1)
  if (start === -1) return null
  const query = before.slice(start + 1)
  const previous = before[start - 1]
  if ((previous !== undefined && !/\s/.test(previous)) || query.includes('\n') || query.length > 48) return null
  return { start, end: before.length, query, trigger: before[start] as MentionTrigger }
}

/** Up to 10 people (for `@`) or channels (for `#`) whose name or handle holds the query, without regard to case. */
export function mentionSuggestions(query: MentionQuery, people: readonly MentionTarget[], channels: readonly MentionTarget[]): MentionSuggestion[] {
  const kind = query.trigger === '#' ? 'channel' : 'user'
  const needle = query.query.trim().toLowerCase()
  return (kind === 'channel' ? channels : people)
    .filter((item) => !needle || item.name.toLowerCase().includes(needle) || (item.handle ?? '').toLowerCase().includes(needle))
    .slice(0, 10)
    .map((item) => ({ id: item.id, label: item.name, detail: kind === 'user' ? (item.handle ?? '') : '', kind }))
}

/** The text after picking a suggestion: `@Name ` (or `#name `) replaces the query, and the cursor goes after it. */
export function applyMention(text: string, query: MentionQuery, suggestion: MentionSuggestion): { text: string; cursor: number } {
  const insertion = `${suggestion.kind === 'channel' ? '#' : '@'}${suggestion.label} `
  return { text: `${text.slice(0, query.start)}${insertion}${text.slice(query.end)}`, cursor: query.start + insertion.length }
}

/** The text after the `@` button: an `@` at the cursor, after a space unless the cursor starts a word. */
export function insertTrigger(text: string, cursor: number): { text: string; cursor: number } {
  const at = Math.min(cursor, text.length)
  const previous = text[at - 1]
  const insertion = previous === undefined || /\s/.test(previous) ? '@' : ' @'
  return { text: `${text.slice(0, at)}${insertion}${text.slice(at)}`, cursor: at + insertion.length }
}

/** Fenced and inline code: `@name` inside it is text, not a mention. */
const CODE = /(```[\s\S]*?```|`[^`\n]+`)/

const isBoundary = (char: string | undefined) => !char || /[\s.,!?;:()[\]{}"'`]/.test(char)

interface Candidate {
  /** The name in lower case. */
  label: string
  /** The length of the name as written: lower case can change the length of a string ("İ"). */
  length: number
  target: MentionMatch['target']
}

export interface MentionMatch {
  /** `[start, end)` in the text: the `@` or `#` and the name. */
  start: number
  end: number
  target: { kind: 'user'; id: string; name: string } | { kind: 'channel'; id: string; name: string } | { kind: 'global'; name: 'channel' | 'here' }
}

/**
 * Every `@Name`, `#name`, `@channel` and `@here` in typed text, in order. A name matches without regard to case, must
 * start and end at a word boundary, and the longest name wins ("@Ada Lovelace" before "@Ada"). Code is left alone.
 */
export function findMentions(text: string, people: readonly MentionTarget[], channels: readonly MentionTarget[]): MentionMatch[] {
  const byLength = (a: Candidate, b: Candidate) => b.label.length - a.label.length
  const globals: Candidate[] = [
    { label: 'channel', length: 7, target: { kind: 'global', name: 'channel' } },
    { label: 'here', length: 4, target: { kind: 'global', name: 'here' } },
  ]
  const users = [
    ...globals,
    ...people.filter((person) => !person.id.startsWith('!')).map((person): Candidate => ({ label: person.name.toLowerCase(), length: person.name.length, target: { kind: 'user', id: person.id, name: person.name } })),
  ]
    .filter((item) => item.label)
    .sort(byLength)
  const rooms = channels
    .map((channel): Candidate => ({ label: channel.name.toLowerCase(), length: channel.name.length, target: { kind: 'channel', id: channel.id, name: channel.name } }))
    .filter((item) => item.label)
    .sort(byLength)
  const matches: MentionMatch[] = []
  let offset = 0
  // `split` with a capturing group puts the code at the odd indexes.
  text.split(CODE).forEach((part, partIndex) => {
    if (partIndex % 2 === 0) {
      let index = 0
      while (index < part.length) {
        const char = part[index]
        const candidates = char === '@' ? users : char === '#' ? rooms : null
        const match =
          candidates && isBoundary(part[index - 1])
            ? candidates.find((item) => part.slice(index + 1, index + 1 + item.length).toLowerCase() === item.label && isBoundary(part[index + 1 + item.length]))
            : undefined
        if (match) {
          matches.push({ start: offset + index, end: offset + index + 1 + match.length, target: match.target })
          index += 1 + match.length
        } else {
          index += 1
        }
      }
    }
    offset += part.length
  })
  return matches
}

const token = (target: MentionMatch['target']) => (target.kind === 'global' ? `<!${target.name}>` : target.kind === 'user' ? `<@${target.id}>` : `<#${target.id}>`)

/** Composer text to a stored chat body: `@Ada Lovelace` becomes `<@u1>`, `#general` `<#c1>`, `@here` `<!here>`. */
export function encodeMentions(text: string, people: readonly MentionTarget[], channels: readonly MentionTarget[]): string {
  let out = ''
  let cursor = 0
  for (const match of findMentions(text, people, channels)) {
    out += text.slice(cursor, match.start) + token(match.target)
    cursor = match.end
  }
  return out + text.slice(cursor)
}

/** The people that typed text mentions by name, each once, in order (a task comment sends them with its markdown). */
export function mentionedUserIds(text: string, people: readonly MentionTarget[]): string[] {
  const ids: string[] = []
  for (const { target } of findMentions(text, people, [])) {
    if (target.kind === 'user' && !ids.includes(target.id)) ids.push(target.id)
  }
  return ids
}

/** One line of a page comment as BlockNote inline content: text runs, and a `mention` node for each `@Name`. */
export function mentionInlineContent(line: string, people: readonly MentionTarget[]): Record<string, unknown>[] {
  const content: Record<string, unknown>[] = []
  const text = (value: string) => {
    if (value) content.push({ type: 'text', text: value, styles: {} })
  }
  let cursor = 0
  for (const { start, end, target } of findMentions(line, people, [])) {
    if (target.kind !== 'user') continue
    text(line.slice(cursor, start))
    content.push({ type: 'mention', props: { userId: target.id, name: target.name } })
    cursor = end
  }
  text(line.slice(cursor))
  return content
}
