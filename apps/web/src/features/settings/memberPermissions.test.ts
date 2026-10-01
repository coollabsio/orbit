import { expect, test } from 'bun:test'
import type { User } from '@/features/workspaces/models'
import { INVITABLE_ROLES, canManageMember, canTransferOwnership } from './memberPermissions'

const member = (id: string, role: User['role']): User => ({
  id, membershipId: `membership-${id}`, name: id, handle: id, email: `${id}@orbit.test`, role,
  color: '#000', online: false, title: '', roleIds: [], version: 1,
})

test('owner is never an ordinary invitation or role change', () => {
  expect(INVITABLE_ROLES).toEqual(['Admin', 'Member'])
  expect(canTransferOwnership(true, 'owner-user', member('member-user', 'Member'))).toBeTrue()
  expect(canTransferOwnership(false, 'admin-user', member('member-user', 'Member'))).toBeFalse()
  expect(canTransferOwnership(true, 'owner-user', member('owner-user', 'Owner'))).toBeFalse()
})

test('owner and admin protections are enforced before rendering member mutations', () => {
  expect(canManageMember(true, 'owner-user', member('admin-user', 'Admin'))).toBeTrue()
  expect(canManageMember(true, 'admin-user', member('owner-user', 'Owner'))).toBeFalse()
  expect(canManageMember(true, 'admin-user', member('member-user', 'Member'))).toBeTrue()
  expect(canManageMember(false, 'member-user', member('admin-user', 'Admin'))).toBeFalse()
  expect(canManageMember(true, 'owner-user', member('owner-user', 'Owner'))).toBeFalse()
})
