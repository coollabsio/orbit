import { expect, test } from 'bun:test'
import { createApiClient } from '@/api/client'
import type { MemberRecord } from '@/api/generated/types.gen'
import { memberFromRecord, workspacesQueryOptions } from './api'

test('workspaces are loaded through the generated client boundary', async () => {
  let path = ''
  const client = createApiClient({
    fetch: async (request) => {
      path = new URL(request.url).pathname
      return Response.json([{ id: 'workspace-1', name: 'Orbit', role: 'owner', permissions: [], version: 1 }])
    },
  })

  const result = await workspacesQueryOptions(client).queryFn()

  expect(path).toBe('/api/v1/workspaces')
  expect(result).toEqual([{ id: 'workspace-1', name: 'Orbit', role: 'owner', permissions: [], version: 1 }])
})

test('member records preserve membership versions for protected mutations', () => {
  const record: MemberRecord = {
    id: 'membership-1', user_id: 'user-1', display_name: 'Ada', email: 'ada@orbit.test',
    role: 'admin', created_at: '2026-09-04T10:00:00Z', version: 4,
    can_change_role: true, can_remove: true, can_transfer_ownership: false,
  }

  expect(memberFromRecord(record)).toMatchObject({
    id: 'user-1', membershipId: 'membership-1', name: 'Ada', email: 'ada@orbit.test',
    role: 'Admin', version: 4,
    can: { changeRole: true, remove: true, transferOwnership: false },
  })
})
