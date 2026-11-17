import { useContext } from 'react'
import type { Permission, WorkspaceRecord } from '@orbit/api/generated/types.gen'
import { useCurrentUser } from '@/features/auth/api'
import { useWorkspace, WorkspaceContext } from './workspaceContext'

/**
 * Whether the caller's role holds `permission` in the current workspace. The list comes with the
 * workspace record (rules: `crates/orbit/src/policy.rs`), so this never makes a request.
 * It only decides what to show: the server checks the same table on every call.
 */
export function useCan(permission: Permission): boolean {
  return useWorkspace().workspace.permissions.includes(permission)
}

/**
 * Whether the caller holds `permission` in one project: workspace-wide, or through a custom role's grant for
 * that project. Mirrors `can_in` in `crates/orbit/src/policy.rs`.
 */
export function canIn(workspace: WorkspaceRecord, permission: Permission, projectId: string | undefined): boolean {
  if (workspace.permissions.includes(permission)) return true
  if (projectId === undefined) return false
  return workspace.project_permissions.some((grant) => grant.project_id === projectId && grant.permissions.includes(permission))
}

/** Mirrors the server's `can_purge_task`: `pages.purge` anywhere, or `tasks.purge` in the task's project. */
export function canPurgeTask(workspace: WorkspaceRecord, projectId: string | undefined): boolean {
  return workspace.permissions.includes('pages.purge') || canIn(workspace, 'tasks.purge', projectId)
}

/**
 * Whether the caller can manage `project` (settings, members, statuses, labels, delete and restore): `projects.manage`
 * there, or being the project's creator. Mirrors the server's project-manage check.
 */
export function canManageProject(workspace: WorkspaceRecord, userId: string | undefined, project: { id: string; created_by?: string | null }): boolean {
  return canIn(workspace, 'projects.manage', project.id) || (!!userId && project.created_by === userId)
}

export function useCanIn(permission: Permission, projectId: string | undefined): boolean {
  return canIn(useWorkspace().workspace, permission, projectId)
}

/**
 * A guest of the current workspace: it sees only Chat, its own channels and the people in them (the server
 * refuses the rest). Outside a workspace (the operator frame) nobody is a guest.
 */
export function useIsGuest(): boolean {
  return useContext(WorkspaceContext)?.workspace.role === 'guest'
}

/** Can open Admin: the root user or an instance admin. */
export function useIsInstallationAdmin(): boolean {
  return useCurrentUser().data?.installation_admin ?? false
}

/** The root user, who also chooses the instance admins. */
export function useIsRoot(): boolean {
  return useCurrentUser().data?.root ?? false
}
