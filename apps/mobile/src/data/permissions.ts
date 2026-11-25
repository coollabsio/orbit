import type { Permission, WorkspaceRecord } from '@orbit/api/generated/types.gen'

/** `time.manage` joins the generated `Permission` when the API types are generated again. */
export type ProjectPermission = Permission | 'time.manage'

type Grants = Pick<WorkspaceRecord, 'permissions'> & Partial<Pick<WorkspaceRecord, 'project_permissions'>>

/** The server's `can_in`: a workspace-wide permission covers every project; a custom role may also grant it in one project. */
export function canInProject(workspace: Grants, permission: ProjectPermission, projectId: string): boolean {
  const holds = (list: readonly ProjectPermission[]) => list.includes(permission)
  return holds(workspace.permissions) || (workspace.project_permissions ?? []).some((grant) => grant.project_id === projectId && holds(grant.permissions))
}

/** The server's `can_purge_task`: `pages.purge` anywhere, or `tasks.purge` in the task's project. */
export function canPurgeTask(workspace: Grants, projectId: string): boolean {
  return workspace.permissions.includes('pages.purge') || canInProject(workspace, 'tasks.purge', projectId)
}
