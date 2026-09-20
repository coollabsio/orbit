// Generic "@mention" / "#channel" helpers shared by chat, docs, tasks and the markdown renderer.
import type { Channel, User } from '@/mock/types'

export type MentionToken = { label: string; color: string; kind: 'user' | 'global' | 'channel'; href?: string }

export function buildMentionTokens(users: User[], channels: Channel[] = []): MentionToken[] {
  return [
    { label: 'everyone', color: '#dee0fc', kind: 'global' },
    { label: 'here', color: '#dee0fc', kind: 'global' },
    ...users.flatMap((user): MentionToken[] => [
      { label: user.name, color: '#dee0fc', kind: 'user' },
      { label: user.handle, color: '#dee0fc', kind: 'user' },
    ]),
    // "#channel" renders like a mention and navigates to the channel
    ...channels.map((channel): MentionToken => ({ label: channel.name, color: '#dee0fc', kind: 'channel', href: `/chat/${channel.id}` })),
  ]
}

export function isMentionBoundary(char: string | undefined): boolean {
  return !char || /\s|[.,!?;:()[\]{}"'`]/.test(char)
}
