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

/**
 * Composer text to the stored body. A name matches without regard to case, must start after a word boundary and end at
 * one, and the longest name wins ("@Ada Lovelace" before "@Ada"). Code is left alone.
 */
export function encodeMentions(text: string, people: readonly MentionTarget[], conversations: readonly MentionTarget[]): string {
  const byLength = (a: { label: string }, b: { label: string }) => b.label.length - a.label.length
  const users = [
    { label: 'channel', token: '<!channel>' },
    { label: 'here', token: '<!here>' },
    ...people.map((person) => ({ label: person.name.toLowerCase(), token: `<@${person.id}>` })),
  ]
    .filter((item) => item.label)
    .sort(byLength)
  const channels = conversations
    .map((conversation) => ({ label: conversation.name.toLowerCase(), token: `<#${conversation.id}>` }))
    .filter((item) => item.label)
    .sort(byLength)

  const encode = (part: string) => {
    const lower = part.toLowerCase()
    let out = ''
    let index = 0
    while (index < part.length) {
      const char = part[index]
      const candidates = char === '@' ? users : char === '#' ? channels : null
      const match =
        candidates && isBoundary(part[index - 1])
          ? candidates.find(
              (item) => lower.startsWith(item.label, index + 1) && isBoundary(part[index + 1 + item.label.length]),
            )
          : undefined
      if (match) {
        out += match.token
        index += 1 + match.label.length
      } else {
        out += char
        index += 1
      }
    }
    return out
  }

  // `split` with a capturing group puts the code segments at the odd indexes.
  return text
    .split(CODE)
    .map((part, index) => (index % 2 === 1 ? part : encode(part)))
    .join('')
}

/** Who a stored body mentions. */
export function extractMentions(body: string): Mentions {
  const mentions: Mentions = { userIds: [], channel: false, here: false }
  for (const match of body.matchAll(TOKEN)) {
    if (match[1] && !mentions.userIds.includes(match[1])) mentions.userIds.push(match[1])
    if (match[3] === 'channel') mentions.channel = true
    if (match[3] === 'here') mentions.here = true
  }
  return mentions
}
