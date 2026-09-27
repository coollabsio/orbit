// Kept free of BlockNote imports: DocEditor (main chunk) uses it, the editor chunk loads lazily.
export interface MentionCandidate {
  id: string
  name: string
  /** Secondary text in the picker (e.g. the email handle). */
  handle?: string
  /** Avatar color (workspace members carry one). */
  color?: string
}

/**
 * Who a comment on this page may mention: every other member for a teamspace page, nobody for a private page (only
 * its owner can see it, and mentioning yourself notifies no one). The server applies the same rule.
 */
export function mentionableMembers<T extends MentionCandidate>(
  members: readonly T[],
  page: { teamspace_id: string | null },
  selfId: string | null,
): T[] {
  if (page.teamspace_id === null) return []
  return members.filter((member) => member.id !== selfId)
}
