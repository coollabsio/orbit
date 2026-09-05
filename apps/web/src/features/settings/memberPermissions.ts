import type { User } from '../tasks/api/models'

export const INVITABLE_ROLES = ['Admin', 'Member'] as const

export function canManageMember(actorRole: string, actorId: string | undefined, target: User): boolean {
  if (!actorId || actorId === target.id) return false
  if (actorRole === 'owner') return true
  return actorRole === 'admin' && target.role !== 'Owner'
}

export function canTransferOwnership(actorRole: string, actorId: string | undefined, target: User): boolean {
  return actorRole === 'owner' && target.id !== actorId && target.role !== 'Owner'
}
