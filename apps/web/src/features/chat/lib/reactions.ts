import type { Reaction } from '../api/types'

/** Adds the user's reaction, or takes it back; a reaction nobody holds any more is dropped. */
export function toggleReaction(reactions: readonly Reaction[], emoji: string, userId: string): Reaction[] {
  const existing = reactions.find((reaction) => reaction.emoji === emoji)
  if (!existing) return [...reactions, { emoji, userIds: [userId] }]
  const userIds = existing.userIds.includes(userId)
    ? existing.userIds.filter((id) => id !== userId)
    : [...existing.userIds, userId]
  return reactions.flatMap((reaction) => (reaction !== existing ? [reaction] : userIds.length > 0 ? [{ emoji, userIds }] : []))
}
