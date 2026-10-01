// Generated from OpenAPI x-role-permissions. Do not edit.
// For test fixtures and mocks only: the app reads `workspace.permissions` from the server.
import type { Permission, WorkspaceRole } from './types.gen'

export const ROLE_PERMISSIONS: Record<WorkspaceRole, Permission[]> = {
  "admin": [
    "workspace.update",
    "members.manage",
    "audit.view",
    "api_tokens.manage",
    "integrations.manage",
    "teamspaces.delete",
    "pages.purge",
    "views.manage_shared",
    "comments.moderate"
  ],
  "member": [],
  "owner": [
    "workspace.update",
    "workspace.delete",
    "workspace.transfer",
    "members.manage",
    "audit.view",
    "api_tokens.manage",
    "integrations.manage",
    "teamspaces.delete",
    "pages.purge",
    "views.manage_shared",
    "comments.moderate"
  ]
}
