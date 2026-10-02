import type { Mentions } from '../api/types'

/** The stored form of a mention. Names are for display only, so a rename never breaks an old message. */
const TOKEN = /<@([^<>\s]+)>|<#([^<>\s]+)>|<!(channel|here)>/g
/** Fenced and inline code: `@name` inside it is text, not a mention. */
const CODE = /(```[\s\S]*?```|`[^`\n]+`)/

export interface MentionTarget {
  id: string
  name: string
}

function isBoundary(char: string | undefined): boolean {
  return !char || /[\s.,!?;:()[\]{}"'`]/.test(char)
}

/** Stored body to the text shown and edited: `<@u1>` becomes `@Ada Lovelace`, `<#c1>` becomes `#general`. */
export function decodeMentions(body: string, people: readonly MentionTarget[], conversations: readonly MentionTarget[]): string {
  return body.replace(TOKEN, (_token, userId?: string, conversationId?: string, global?: string) => {
    if (global) return `@${global}`
    if (userId) return `@${people.find((person) => person.id === userId)?.name ?? 'unknown'}`
    return `#${conversations.find((conversation) => conversation.id === conversationId)?.name ?? 'unknown'}`
  })
}

interface MentionCandidate {
  /** The name in lower case. */
  label: string
  /** The length of the name as written: lower case can change the length of a string ("İ"). */
  length: number
  token: string
}

/** What `@…` and `#…` can match, longest name first so "Ann Lee" wins over "Ann". */
function mentionCandidates(people: readonly MentionTarget[], conversations: readonly MentionTarget[]) {
  const byLength = (a: MentionCandidate, b: MentionCandidate) => b.label.length - a.label.length
  const users = [
    { label: 'channel', length: 7, token: '<!channel>' },
    { label: 'here', length: 4, token: '<!here>' },
    ...people.map((person) => ({ label: person.name.toLowerCase(), length: person.name.length, token: `<@${person.id}>` })),
  ]
    .filter((item) => item.label)
    .sort(byLength)
  const channels = conversations
    .map((conversation) => ({ label: conversation.name.toLowerCase(), length: conversation.name.length, token: `<#${conversation.id}>` }))
    .filter((item) => item.label)
    .sort(byLength)
  return { users, channels }
}

export interface MentionMatch {
  /** `[start, end)` in the text: the `@` or `#` and the name. */
  start: number
  end: number
  token: string
}

/** Every `@Name`, `#name`, `@channel` and `@here` in typed text, in order. Text inside code is not a mention. */
export function findMentions(text: string, people: readonly MentionTarget[], conversations: readonly MentionTarget[]): MentionMatch[] {
  const { users, channels } = mentionCandidates(people, conversations)
  const matches: MentionMatch[] = []
  let offset = 0
  // `split` with a capturing group puts the code segments at the odd indexes.
  text.split(CODE).forEach((part, partIndex) => {
    if (partIndex % 2 === 0) {
      let index = 0
      while (index < part.length) {
        const at = offset + index
        const char = part[index]
        const candidates = char === '@' ? users : char === '#' ? channels : null
        const match =
          candidates && isBoundary(part[index - 1])
            ? candidates.find((item) => part.slice(index + 1, index + 1 + item.length).toLowerCase() === item.label && isBoundary(part[index + 1 + item.length]))
            : undefined
        if (match) {
          matches.push({ start: at, end: at + 1 + match.length, token: match.token })
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

/**
 * Composer text to the stored body. A name matches without regard to case, must start after a word boundary and end at
 * one, and the longest name wins ("@Ada Lovelace" before "@Ada"). Code is left alone.
 */
export function encodeMentions(text: string, people: readonly MentionTarget[], conversations: readonly MentionTarget[]): string {
  let out = ''
  let cursor = 0
  for (const match of findMentions(text, people, conversations)) {
    out += text.slice(cursor, match.start) + match.token
    cursor = match.end
  }
  return out + text.slice(cursor)
}

export function extractMentions(body: string): Mentions {
  const mentions: Mentions = { userIds: [], channel: false, here: false }
  for (const match of body.matchAll(TOKEN)) {
    if (match[1] && !mentions.userIds.includes(match[1])) mentions.userIds.push(match[1])
    if (match[3] === 'channel') mentions.channel = true
    if (match[3] === 'here') mentions.here = true
  }
  return mentions
}
