import type { Permission } from '@orbit/api/generated/types.gen'
import { useCurrentUser } from '@/features/auth/api'
import { useWorkspace } from './workspaceContext'

/**
 * Whether the caller's role holds `permission` in the current workspace. The list comes with the
 * workspace record (rules: `crates/orbit/src/policy.rs`), so this never makes a request.
 * It only decides what to show: the server checks the same table on every call.
 */
export function useCan(permission: Permission): boolean {
  return useWorkspace().workspace.permissions.includes(permission)
}

/** Can open Admin: the root user or an instance admin. */
export function useIsInstallationAdmin(): boolean {
  return useCurrentUser().data?.installation_admin ?? false
}

/** The root user, who also chooses the instance admins. */
export function useIsRoot(): boolean {
  return useCurrentUser().data?.root ?? false
}
