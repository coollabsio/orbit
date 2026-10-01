import { ROLE_PERMISSIONS } from '@/api/generated/rolePermissions'
import type { WorkspaceRecord, WorkspaceRole } from '@/api/generated/types.gen'

/** A workspace record as the server sends it to a caller with `role`. */
export function testWorkspace(role: WorkspaceRole = 'owner', overrides: Partial<WorkspaceRecord> = {}): WorkspaceRecord {
  return { id: 'workspace-1', name: 'Orbit', role, permissions: ROLE_PERMISSIONS[role], version: 1, ...overrides }
}
