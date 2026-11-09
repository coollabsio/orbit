import type { InvitationBody } from '@orbit/api/generated/types.gen'

/** The roles an invitation gives. Owner is never one: that role only moves through an ownership transfer. */
export const INVITABLE_ROLES = ['Admin', 'Member', 'Guest'] as const
export type InvitableRole = (typeof INVITABLE_ROLES)[number]

/**
 * The request for an invitation, or why the form cannot send it yet. A guest needs one or more private channels; the
 * other roles send none.
 */
export function invitationRequest(
  email: string,
  role: InvitableRole,
  channelIds: readonly string[],
  delivery: InvitationBody['delivery'],
): { body: InvitationBody } | { problem: string } {
  if (role === 'Guest' && channelIds.length === 0) return { problem: 'Choose one or more channels for the guest.' }
  const body: InvitationBody = { email, role: role === 'Admin' ? 'admin' : role === 'Guest' ? 'guest' : 'member', delivery }
  return { body: role === 'Guest' ? { ...body, conversation_ids: [...channelIds] } : body }
}
