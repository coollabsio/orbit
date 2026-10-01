import type { Permission, WorkspaceRecord, WorkspaceRole } from '@/api/generated/types.gen'

const ADMIN: Permission[] = [
  'workspace.update', 'members.manage', 'audit.view', 'api_tokens.manage', 'integrations.manage',
  'teamspaces.delete', 'pages.purge', 'views.manage_shared', 'comments.moderate',
]

// Test copy of the role table in `crates/orbit/src/policy.rs`; the app itself only reads what the server sends.
const PERMISSIONS: Record<WorkspaceRole, Permission[]> = {
  owner: [...ADMIN, 'workspace.delete', 'workspace.transfer'],
  admin: ADMIN,
  member: [],
}

/** A workspace record as the server sends it to a caller with `role`. */
export function testWorkspace(role: WorkspaceRole = 'owner', overrides: Partial<WorkspaceRecord> = {}): WorkspaceRecord {
  return { id: 'workspace-1', name: 'Orbit', role, permissions: PERMISSIONS[role], version: 1, ...overrides }
}
