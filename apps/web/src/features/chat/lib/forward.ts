import type { Conversation, Message } from '../api/types'

/** How many conversations one forward can go to (Discord's limit). */
export const FORWARD_LIMIT = 5

/** A confirmed message that is not deleted can be forwarded: also a thread reply, and a forward (it keeps its first origin). */
export function canForward(message: Message): boolean {
  return message.kind === 'message' && !message.deleted && !message.sendState
}

/**
 * The user's own confirmed message can be edited. A forward cannot: it is a copy of what somebody else wrote. Nor can
 * a message that is only a sticker: it has no text to change.
 */
export function canEdit(message: Message, currentUserId: string | null): boolean {
  const stickerOnly = Boolean(message.stickerId) && message.body === ''
  return canForward(message) && !message.forwarded && !stickerOnly && message.authorId === currentUserId
}

/**
 * Where a message can be forwarded to: the channels the user is in and the DMs, without archived ones, latest activity
 * first. `search` keeps those whose title contains it, without regard to case; a `#` at its start is ignored.
 */
export function forwardDestinations(conversations: readonly Conversation[], titleOf: (conversation: Conversation) => string, search = ''): Conversation[] {
  const query = search.trim().toLowerCase().replace(/^#/, '')
  const activity = (conversation: Conversation) => conversation.lastMessageAt ?? conversation.createdAt
  return conversations
    .filter((conversation) => conversation.isMember && !conversation.archived)
    .filter((conversation) => !query || titleOf(conversation).toLowerCase().replace(/^#/, '').includes(query))
    .sort((a, b) => activity(b) - activity(a))
}

/** Selects a destination or takes it out. One more than `FORWARD_LIMIT` changes nothing. */
export function toggleDestination(selectedIds: readonly string[], conversationId: string): string[] {
  if (selectedIds.includes(conversationId)) return selectedIds.filter((id) => id !== conversationId)
  return selectedIds.length < FORWARD_LIMIT ? [...selectedIds, conversationId] : [...selectedIds]
}

export interface ForwardResult {
  /** The destinations that have the forward, in order. */
  sentIds: string[]
  /** What the first failed destination was refused with. Those after it were not tried. */
  error?: unknown
}

/**
 * Forwards to each destination in turn. `comment` sends the user's own message to a destination once its forward is
 * there, so it lands right after it. The first failure stops the run.
 */
export async function forwardToEach(
  conversationIds: readonly string[],
  forward: (conversationId: string) => Promise<unknown>,
  comment?: (conversationId: string) => void,
): Promise<ForwardResult> {
  const sentIds: string[] = []
  for (const conversationId of conversationIds) {
    try {
      await forward(conversationId)
    } catch (error) {
      return { sentIds, error }
    }
    comment?.(conversationId)
    sentIds.push(conversationId)
  }
  return { sentIds }
}
