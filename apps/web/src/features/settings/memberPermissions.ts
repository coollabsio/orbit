import type { User } from '@/features/workspaces/models'

export const INVITABLE_ROLES = ['Admin', 'Member'] as const

/** `canManageMembers` is the `members.manage` permission. Nobody manages themselves or the owner. */
export function canManageMember(canManageMembers: boolean, actorId: string | undefined, target: User): boolean {
  return canManageMembers && !!actorId && actorId !== target.id && target.role !== 'Owner'
}

/** `canTransfer` is the `workspace.transfer` permission. */
export function canTransferOwnership(canTransfer: boolean, actorId: string | undefined, target: User): boolean {
  return canTransfer && target.id !== actorId && target.role !== 'Owner'
}
