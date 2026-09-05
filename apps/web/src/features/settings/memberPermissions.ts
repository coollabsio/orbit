import type { User } from '../tasks/api/models'

export function canManageMember(actorRole: string, actorId: string | undefined, target: User): boolean {
  if (!actorId || actorId === target.id) return false
  if (actorRole === 'owner') return true
  return actorRole === 'admin' && target.role !== 'Owner'
}
