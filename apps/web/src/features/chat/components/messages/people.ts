import { useMembers } from '@/features/workspaces/api'
import type { User } from '@/features/workspaces/models'
import { buildMentionTokens, type MentionToken } from '@/lib/mentions'
import { useChatContext } from '../../api/chatContext'
import { useConversations } from '../../api/queries'
import type { Conversation } from '../../api/types'
import type { MentionTarget } from '../../lib/mentionTokens'

export interface ChatPeople {
  members: User[]
  byId: ReadonlyMap<string, User>
  /** Channels (not DMs) that `#name` can point to. */
  channels: MentionTarget[]
  /** For the markdown renderer: it highlights `@Name` and `#name` by label. */
  tokens: MentionToken[]
}

const NO_MEMBERS: User[] = []
const NO_CONVERSATIONS: Conversation[] = []
/** One derived value for each (members, conversations) pair, so every message row shares it. */
const cache = new WeakMap<User[], WeakMap<Conversation[], ChatPeople>>()

function derive(members: User[], conversations: Conversation[]): ChatPeople {
  const channels = conversations.filter((conversation) => conversation.kind !== 'dm').map(({ id, name }) => ({ id, name }))
  return {
    members,
    byId: new Map(members.map((member) => [member.id, member])),
    channels,
    tokens: buildMentionTokens(members, channels),
  }
}

/** The workspace's members and channels in the shapes that message rendering and the composer need. */
export function useChatPeople(): ChatPeople {
  const { workspaceId } = useChatContext()
  const members = useMembers(workspaceId).data ?? NO_MEMBERS
  const conversations = useConversations().data ?? NO_CONVERSATIONS
  let byConversations = cache.get(members)
  if (!byConversations) {
    byConversations = new WeakMap()
    cache.set(members, byConversations)
  }
  let people = byConversations.get(conversations)
  if (!people) {
    people = derive(members, conversations)
    byConversations.set(conversations, people)
  }
  return people
}

/** The title of a conversation: `#name` for a channel, the other members' names for a DM. */
export function conversationTitle(conversation: Conversation, people: ChatPeople, currentUserId: string | null): string {
  if (conversation.kind !== 'dm') return `#${conversation.name}`
  const others = conversation.memberIds.filter((id) => id !== currentUserId)
  const names = (others.length > 0 ? others : conversation.memberIds).map((id) => people.byId.get(id)?.name ?? 'Unknown')
  return names.join(', ')
}
