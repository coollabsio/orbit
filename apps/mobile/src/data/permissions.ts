import type { Permission, WorkspaceRecord } from '@orbit/api/generated/types.gen'

type Grants = Pick<WorkspaceRecord, 'permissions'> & Partial<Pick<WorkspaceRecord, 'project_permissions'>>

/** The server's `can_in`: a workspace-wide permission covers every project; a custom role may also grant it in one project. */
export function canInProject(workspace: Grants, permission: Permission, projectId: string): boolean {
  return (
    workspace.permissions.includes(permission) ||
    (workspace.project_permissions ?? []).some((grant) => grant.project_id === projectId && grant.permissions.includes(permission))
  )
}

/** The server's `can_purge_task`: `pages.purge` anywhere, or `tasks.purge` in the task's project. */
export function canPurgeTask(workspace: Grants, projectId: string): boolean {
  return workspace.permissions.includes('pages.purge') || canInProject(workspace, 'tasks.purge', projectId)
}
