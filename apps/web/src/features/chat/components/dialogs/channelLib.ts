import { ChatError, type ChatErrorCode, type Conversation } from '@/features/chat/api/types'

export const CHANNEL_NAME_MAX = 80

/**
 * A channel name while it is typed: lowercase, spaces become hyphens. A hyphen at the end stays, so the next word can
 * follow it; `channelNameFinal` removes it on submit.
 */
export function channelNameInput(value: string): string {
  return value
    .toLowerCase()
    .replace(/\s+/g, '-')
    .replace(/-{2,}/g, '-')
    .replace(/^[-#]+/, '')
    .slice(0, CHANNEL_NAME_MAX)
}

/** The name that is sent: no hyphen at the end. Empty when there is nothing to send. */
export function channelNameFinal(value: string): string {
  return channelNameInput(value).replace(/-+$/, '')
}

interface Person {
  id: string
  role: 'Owner' | 'Admin' | 'Member' | 'Guest'
  suspended?: boolean
}

/** The creator of a channel and workspace owners and admins can edit it, archive it and (private) manage its members. */
export function canManageChannel(conversation: Pick<Conversation, 'kind' | 'createdBy'>, user: Pick<Person, 'id' | 'role'> | undefined): boolean {
  if (!user || conversation.kind === 'dm') return false
  return user.role === 'Owner' || user.role === 'Admin' || conversation.createdBy === user.id
}

/** "Add people" shows to those who manage a private channel and to every member of a public one; never in `#general` or a DM. */
export function canAddPeople(
  conversation: Pick<Conversation, 'kind' | 'createdBy' | 'isMember' | 'isDefault' | 'archived'>,
  user: Pick<Person, 'id' | 'role'> | undefined,
): boolean {
  if (conversation.kind === 'dm' || conversation.isDefault || conversation.archived) return false
  return conversation.kind === 'private' ? canManageChannel(conversation, user) : conversation.isMember
}

/** People a picker can offer: not the current user, not suspended, and not the ones in `excludeIds`. By name. */
export function pickablePeople<T extends Person & { name: string }>(
  members: readonly T[],
  currentUserId: string | null,
  excludeIds: readonly string[] = [],
): T[] {
  return members
    .filter((member) => member.id !== currentUserId && !member.suspended && !excludeIds.includes(member.id))
    .sort((a, b) => a.name.localeCompare(b.name))
}

/** Who "Add people" offers for a channel: the people not in it yet, and never a guest for a public channel. */
export function addablePeople<T extends Person & { name: string }>(
  conversation: Pick<Conversation, 'kind' | 'memberIds'>,
  members: readonly T[],
  currentUserId: string | null,
): T[] {
  const allowed = conversation.kind === 'public' ? members.filter((member) => member.role !== 'Guest') : members
  return pickablePeople(allowed, currentUserId, conversation.memberIds)
}

/**
 * People a new message can go to: yourself first, as "Name (you)" (the DM with yourself), then `pickablePeople`. As the
 * server requires, a guest is offered only when it shares an open channel with you (and, when you are a guest, everybody is).
 */
export function messageablePeople<T extends Person & { name: string }>(
  members: readonly T[],
  conversations: readonly Pick<Conversation, 'kind' | 'archived' | 'memberIds'>[],
  currentUserId: string | null,
): T[] {
  const me = members.find((member) => member.id === currentUserId)
  const shared = new Set(
    conversations
      .filter((conversation) => conversation.kind !== 'dm' && !conversation.archived && currentUserId !== null && conversation.memberIds.includes(currentUserId))
      .flatMap((conversation) => conversation.memberIds),
  )
  const reachable = members.filter((member) => (me?.role !== 'Guest' && member.role !== 'Guest') || shared.has(member.id))
  const others = pickablePeople(reachable, currentUserId)
  return me ? [{ ...me, name: `${me.name} (you)` }, ...others] : others
}

/** Name, handle or email contains the text, without regard to case. An empty text matches everybody. */
export function matchPeople<T extends { name: string; handle: string; email: string }>(people: readonly T[], text: string): T[] {
  const query = text.trim().toLowerCase().replace(/^@/, '')
  if (!query) return [...people]
  return people.filter((person) => [person.name, person.handle, person.email].some((field) => field.toLowerCase().includes(query)))
}

const GENERAL_MESSAGES: Partial<Record<ChatErrorCode, string>> = {
  forbidden: 'You do not have permission to do this.',
  offline: 'You are offline. Try again when you are connected.',
}

/**
 * A plain sentence for a failed write. `byCode` holds the sentences of this action; `fallback` names the action
 * ("The channel was not created. Try again.").
 */
export function chatErrorMessage(error: unknown, fallback: string, byCode: Partial<Record<ChatErrorCode, string>> = {}): string {
  if (!(error instanceof ChatError)) return fallback
  return byCode[error.code] ?? GENERAL_MESSAGES[error.code] ?? fallback
}
