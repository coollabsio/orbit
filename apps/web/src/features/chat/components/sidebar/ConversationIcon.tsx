import { Hashtag, Lock } from 'reicon-react'
import { UserAvatar, UserAvatarStack } from '@/components/common/UserAvatar'
import type { Conversation } from '@/features/chat/api/types'
import { dmPeerIds, isSelfDm } from '@/features/chat/lib/sidebar'
import { presenceOf, usePresence } from '@/features/realtime/presence'
import type { User } from '@/features/workspaces/models'

/**
 * The glyph of a conversation: `#` or a lock for a channel, the other person's avatar with presence for a 1:1 DM (your
 * own avatar, without presence, for the DM with yourself), and
 * for a group DM stacked avatars (`group="stack"`, the header) or the number of other members (`group="count"`, a row).
 */
export function ConversationIcon({
  conversation,
  people,
  currentUserId,
  size = 18,
  group = 'count',
  filled = false,
}: {
  conversation: Conversation
  people: readonly User[]
  currentUserId: string | null
  size?: number
  group?: 'stack' | 'count'
  filled?: boolean
}) {
  const presence = usePresence()
  if (conversation.kind !== 'dm') {
    const Icon = conversation.kind === 'private' ? Lock : Hashtag
    return (
      <span data-slot="conversation-icon" className="flex shrink-0 items-center justify-center text-muted-foreground" style={{ width: size, height: size }}>
        <Icon size={Math.round(size * 0.9)} weight={filled ? 'Filled' : 'Outline'} aria-hidden="true" />
        {conversation.kind === 'private' ? <span className="sr-only">Private channel</span> : null}
      </span>
    )
  }
  const peerIds = dmPeerIds(conversation, currentUserId ?? '')
  if (peerIds.length <= 1) {
    // the DM with yourself shows your avatar without a presence dot; a DM whose other members left has no avatar
    const id = peerIds[0] ?? (isSelfDm(conversation) ? currentUserId : undefined)
    return <UserAvatar user={people.find((person) => person.id === id)} size={size} status={peerIds.length === 1 ? presenceOf(presence, id).status : undefined} />
  }
  if (group === 'stack') return <UserAvatarStack users={people.filter((person) => peerIds.includes(person.id))} size={size} />
  return (
    <span
      data-slot="conversation-icon"
      aria-hidden="true"
      className="flex shrink-0 items-center justify-center rounded-[5px] bg-muted text-[10px] leading-none font-semibold text-muted-foreground tabular-nums"
      style={{ width: size, height: size }}
    >
      {peerIds.length}
    </span>
  )
}
