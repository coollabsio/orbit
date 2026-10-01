import type { Permission } from '@/api/generated/types.gen'
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

/** Installation administrators manage backups, the global audit log and account suspension. */
export function useIsInstallationAdmin(): boolean {
  return useCurrentUser().data?.installation_admin ?? false
}
