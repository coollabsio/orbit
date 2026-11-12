/**
 * `@` and `#` mentions in a plain text field, as the web composer does them (`useMentionAutocomplete` and
 * `mentionTokens` there): where the open mention is, what it suggests, the text after a pick, and the typed `@Name`
 * matches that are sent as mentions.
 */

import { findTaskIds, taskUrl } from './taskIds'

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
  /** The handle of a person; a note such as "everyone here" for `@channel`; a task's identifier (`ENG-12`). */
  detail: string
  kind: 'user' | 'channel' | 'task'
  /** A task's status and project name, for its row. */
  status?: { category: string; color: string }
  project?: string
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

/**
 * The text after picking a suggestion: `@Name ` (or `#name `) replaces the query, and the cursor goes after it. A task
 * goes in as its identifier (`ENG-12 `), which the web shows as the task's chip.
 */
export function applyMention(text: string, query: MentionQuery, suggestion: MentionSuggestion): { text: string; cursor: number } {
  const insertion = suggestion.kind === 'task' ? `${suggestion.detail} ` : `${suggestion.kind === 'channel' ? '#' : '@'}${suggestion.label} `
  return { text: `${text.slice(0, query.start)}${insertion}${text.slice(query.end)}`, cursor: query.start + insertion.length }
}

/** The text after the `@` or `#` button: the trigger at the cursor, after a space unless the cursor starts a word. */
export function insertTrigger(text: string, cursor: number, trigger: '@' | '#'): { text: string; cursor: number } {
  const at = Math.min(cursor, text.length)
  const previous = text[at - 1]
  const insertion = previous === undefined || /\s/.test(previous) ? trigger : ` ${trigger}`
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

/** What turns typed task IDs into links: the workspace's project keys and the server's address. */
export interface TaskLinking {
  keys: readonly string[]
  server: string
}

/**
 * One line of a page comment as BlockNote inline content: text runs, a `mention` node for each `@Name`, and a link to
 * the task for each typed `ENG-12` (with `tasks`).
 */
export function mentionInlineContent(line: string, people: readonly MentionTarget[], tasks?: TaskLinking): Record<string, unknown>[] {
  const content: Record<string, unknown>[] = []
  const plain = (value: string) => {
    if (value) content.push({ type: 'text', text: value, styles: {} })
  }
  const text = (value: string) => {
    let at = 0
    for (const id of tasks ? findTaskIds(value, tasks.keys) : []) {
      plain(value.slice(at, id.start))
      content.push({ type: 'link', href: taskUrl(tasks!.server, id.identifier), content: [{ type: 'text', text: value.slice(id.start, id.end), styles: {} }] })
      at = id.end
    }
    plain(value.slice(at))
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

/** `ENG-12`, `eng-`, `#12` or `12`: the user is typing an identifier, so tasks lead (the web's `looksLikeTaskId`). */
export function looksLikeTaskId(query: string): boolean {
  const text = query.trim()
  return (/^[A-Za-z0-9_]{1,20}-\d*$/.test(text) && /[A-Za-z_]/.test(text)) || /^#?\d+$/.test(text)
}

/**
 * The text after "#" that may name a task, or null when it cannot: it starts with a space (`# Heading`) or is long, so
 * plain text typed after a stray "#" stops searching. Empty: nothing typed yet (recently updated tasks).
 */
export function taskSearchText(query: string): string | null {
  if (/^\s/.test(query) || query.trim().split(/\s+/).length > 4) return null
  return query.trim()
}

/** Best task hit first: the exact identifier, an identifier that starts with the query, then title hits (the web's order). */
export function rankTaskSuggestions(tasks: readonly MentionSuggestion[], query: string): MentionSuggestion[] {
  const needle = query.trim().toLowerCase().replace(/^#/, '')
  if (!needle) return [...tasks]
  const rank = (task: MentionSuggestion) => {
    const id = task.detail.toLowerCase()
    const title = task.label.toLowerCase()
    if (id === needle || (/^\d+$/.test(needle) && id.endsWith(`-${needle}`))) return 0
    if (looksLikeTaskId(needle) && id.startsWith(needle)) return 1
    if (title.startsWith(needle)) return 2
    if (title.includes(needle)) return 3
    return 4
  }
  return tasks
    .map((task, index) => ({ task, index, rank: rank(task) }))
    .sort((a, b) => a.rank - b.rank || a.index - b.index)
    .map(({ task }) => task)
}

/**
 * Channels and tasks (the "#" list) in one list of at most 10: tasks first when the query looks like an identifier,
 * channels first otherwise. A kind with fewer hits leaves its room to the other (the web's `mixSuggestions`).
 */
export function mixSuggestions(others: MentionSuggestion[], tasks: MentionSuggestion[], query: string): MentionSuggestion[] {
  if (tasks.length === 0) return others.slice(0, 10)
  const shownOthers = others.slice(0, Math.max(4, 10 - Math.min(tasks.length, 6)))
  const shownTasks = tasks.slice(0, 10 - shownOthers.length)
  return looksLikeTaskId(query) ? [...shownTasks, ...shownOthers] : [...shownOthers, ...shownTasks]
}
