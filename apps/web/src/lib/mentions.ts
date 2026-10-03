// Generic "@mention" / "#channel" helpers shared by chat, docs, tasks and the markdown renderer.

/** What a mention needs of a person. A workspace member fits. */
export interface MentionPerson {
  id: string
  name: string
  handle: string
}

/** What a "#channel" mention needs of a channel. */
export interface MentionChannel {
  id: string
  name: string
}

export type MentionToken = {
  label: string
  color: string
  kind: 'user' | 'global' | 'channel'
  href?: string
  /** The member a `user` token names. Absent when two members share the label: the mention then opens no profile. */
  userId?: string
}

export function buildMentionTokens(users: MentionPerson[], channels: MentionChannel[] = []): MentionToken[] {
  // a label names one member only when no other member has it as a name or handle
  const owners = new Map<string, string | null>()
  for (const user of users) {
    for (const label of [user.name, user.handle]) {
      const key = label.toLowerCase()
      const owner = owners.get(key)
      owners.set(key, owner === undefined || owner === user.id ? user.id : null)
    }
  }
  const userToken = (label: string): MentionToken => ({ label, color: '#dee0fc', kind: 'user', userId: owners.get(label.toLowerCase()) ?? undefined })
  return [
    { label: 'channel', color: '#dee0fc', kind: 'global' },
    { label: 'here', color: '#dee0fc', kind: 'global' },
    ...users.flatMap((user): MentionToken[] => [userToken(user.name), userToken(user.handle)]),
    // "#channel" renders like a mention and navigates to the channel
    ...channels.map((channel): MentionToken => ({ label: channel.name, color: '#dee0fc', kind: 'channel', href: `/chat/${channel.id}` })),
  ]
}

/**
 * The member that the text `@label` names, the way the renderer recognises a mention: by name or handle, without
 * regard to case. `null` for `@channel`, `@here`, an unknown name, and a name that more than one member has.
 */
export function mentionedUserId(label: string, tokens: readonly MentionToken[]): string | null {
  const key = label.toLowerCase()
  const token = tokens.find((candidate) => candidate.kind !== 'channel' && candidate.label.toLowerCase() === key)
  return token?.kind === 'user' ? (token.userId ?? null) : null
}

export function isMentionBoundary(char: string | undefined): boolean {
  return !char || /\s|[.,!?;:()[\]{}"'`]/.test(char)
}
