import type { Message } from '../api/types'

/**
 * A message that the current user wrote as themself. An integration's message is nobody's own, also not of the member
 * who set the integration up (its `authorId`): it counts as unread for them, and they cannot edit it.
 */
export function isOwnMessage(message: Pick<Message, 'authorId' | 'bot'>, currentUserId: string | null): boolean {
  return !message.bot && message.authorId === currentUserId
}
