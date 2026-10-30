/**
 * The account and workspace settings calls (profile picture, status, password, two-factor, sessions, notification choices,
 * workspace name, storage, members and invitations) over the generated SDK, with the pure rules the screens share. No
 * React.
 */
import {
  authOptions as authOptionsRequest,
  changeMemberRole,
  changePassword as changePasswordRequest,
  createInvitation,
  deleteAccount as deleteAccountRequest,
  deleteStorageFile as deleteStorageFileRequest,
  disableTwoFactor,
  enableTotp,
  getNotificationPreferences,
  getTwoFactor,
  getWorkspaceBucket,
  getWorkspaceStorage,
  listInvitations,
  listSessions,
  putStatus,
  putNotificationPreferences,
  regenerateRecoveryCodes,
  removeAvatar,
  removeMember,
  removeWorkspaceBucket,
  renameWorkspace as renameWorkspaceRequest,
  requestEmailChange as requestEmailChangeRequest,
  revokeInvitation as revokeInvitationRequest,
  revokeSession as revokeSessionRequest,
  saveWorkspaceBucket,
  setupTotp,
  transferOwnership as transferOwnershipRequest,
  uploadAvatar,
} from '@orbit/api/generated/sdk.gen'
import type {
  AuthOptions,
  AuthProblem,
  OwnedWorkspace,
  InvitationRecord,
  NotificationPrefs,
  S3Body,
  SessionRecord,
  TotpSetup,
  TwoFactorStatus,
  UserStatus,
  WorkspaceBucketView,
  WorkspaceRole,
  WorkspaceStorage,
} from '@orbit/api/generated/types.gen'

import { ApiProblem } from '@orbit/api/problem'

import type { MemberRecord, OrbitClient, User, WorkspaceRecord } from './orbit'
import type { OAuthConnection } from '@orbit/api/mcp'
import { STORAGE_FILES_PAGE } from '@orbit/api/storage'

export type { InvitationRecord, NotificationPrefs, OwnedWorkspace, SessionRecord, TotpSetup, TwoFactorStatus, WorkspaceBucketView, WorkspaceStorage }

/** Query keys of the account (not per workspace) and of the workspace settings. */
export const accountKeys = {
  notificationPrefs: ['account', 'notification-prefs'] as const,
  sessions: ['account', 'sessions'] as const,
  twoFactor: ['account', 'two-factor'] as const,
  connectedAccounts: ['account', 'connected-accounts'] as const,
  oauthConnections: ['account', 'oauth-connections'] as const,
  /** Under the `members` prefix, so the workspace events socket refreshes it with the members. */
  invitations: (workspaceId: string) => ['members', workspaceId, 'invitations'] as const,
  storage: (workspaceId: string) => ['workspace-storage', workspaceId] as const,
  bucket: (workspaceId: string) => ['workspace-bucket', workspaceId] as const,
  authOptions: ['auth-options'] as const,
}

/** A file for a multipart upload: a `Blob` in a browser or Bun, an expo-file-system `File` in the app (`src/upload-part.ts`). */
export type UploadFile = Blob

/** The form with the file. A plain Blob has no name, so it is sent as "upload"; the app's `File` brings its own. */
export function multipart(file: UploadFile) {
  return () => {
    const form = new FormData()
    if (file instanceof Blob) form.append('file', file, 'upload')
    else form.append('file', file)
    return form
  }
}

/** The name a picked photo is uploaded under: the picker's file name, else one from its type. */
export function pickedImageName(asset: { uri: string; fileName?: string | null; mimeType?: string | null }): string {
  const type = asset.mimeType ?? (/\.png$/i.test(asset.uri) ? 'image/png' : 'image/jpeg')
  const extension = type.split('/')[1] ?? 'jpg'
  return asset.fileName ?? `picture.${extension === 'jpeg' ? 'jpg' : extension}`
}

// ---- profile picture and password

export async function uploadProfilePicture(client: OrbitClient, file: UploadFile): Promise<User> {
  return (await uploadAvatar({ client, body: { file: file as Blob }, bodySerializer: multipart(file), throwOnError: true })).data
}

export async function removeProfilePicture(client: OrbitClient): Promise<User> {
  return (await removeAvatar({ client, throwOnError: true })).data
}

export async function changePassword(client: OrbitClient, currentPassword: string, newPassword: string): Promise<void> {
  await changePasswordRequest({ client, body: { current_password: currentPassword, new_password: newPassword }, throwOnError: true })
}

/** The web's check before the request: the problem to show, or `null` to send it. */
export function passwordProblem(current: string, next: string, confirm: string): string | null {
  if (!current || !next || !confirm) return 'Fill in all three fields.'
  if (next !== confirm) return 'The new password confirmation does not match.'
  return null
}

// ---- email and deleting the account

/**
 * Emails a link to the new address; the email changes when it is opened (on the web). `password` is left out for an
 * account without one (see `withReauthentication`).
 */
export async function requestEmailChange(client: OrbitClient, email: string, password?: string): Promise<void> {
  await requestEmailChangeRequest({ client, body: { email, password }, throwOnError: true })
}

/** Deletes the account for good; `email` is the typed confirmation. Every session ends. */
export async function deleteAccount(client: OrbitClient, email: string, password?: string): Promise<void> {
  await deleteAccountRequest({ client, body: { email, password }, throwOnError: true })
}

/** The typed confirmation matches the account's email, as the server compares them (case and spaces aside). */
export const confirmsEmail = (typed: string, email: string) => typed.trim().toLowerCase() === email.trim().toLowerCase()

/** 409 `owns_workspaces` lists the workspaces to delete first in the problem body; `null` for any other error. */
export function ownedWorkspaces(error: unknown): OwnedWorkspace[] | null {
  if (!(error instanceof ApiProblem) || error.code !== 'owns_workspaces') return null
  const { workspaces } = error.problem as AuthProblem
  return Array.isArray(workspaces) ? workspaces : []
}

// ---- status

/** Sets the presence and the custom status; answers with the user. */
export async function setStatus(client: OrbitClient, status: UserStatus): Promise<User> {
  return (await putStatus({ client, body: status, throwOnError: true })).data
}

// ---- two-factor sign-in

export async function twoFactor(client: OrbitClient): Promise<TwoFactorStatus> {
  return (await getTwoFactor({ client, throwOnError: true })).data
}

/** `password` is left out for an account without one (see `withReauthentication`). */
export async function startTotpSetup(client: OrbitClient, password?: string): Promise<TotpSetup> {
  return (await setupTotp({ client, body: { password }, throwOnError: true })).data
}

/** Turns the authenticator app on; answers with the recovery codes. */
export async function turnOnTotp(client: OrbitClient, code: string): Promise<string[]> {
  return (await enableTotp({ client, body: { code }, throwOnError: true })).data.recovery_codes
}

export async function turnOffTwoFactor(client: OrbitClient, password?: string): Promise<void> {
  await disableTwoFactor({ client, body: { password }, throwOnError: true })
}

export async function newRecoveryCodes(client: OrbitClient, password?: string): Promise<string[]> {
  return (await regenerateRecoveryCodes({ client, body: { password }, throwOnError: true })).data.recovery_codes
}

/** `On · 3 recovery codes left` or the web's off wording. */
export function twoFactorSummary(status: TwoFactorStatus): string {
  if (!status.totp_enabled) return 'Only your password is needed to sign in.'
  const left = status.recovery_codes_left
  return `On · ${left} recovery ${left === 1 ? 'code' : 'codes'} left`
}

// ---- sessions

export async function sessions(client: OrbitClient): Promise<SessionRecord[]> {
  return (await listSessions({ client, throwOnError: true })).data
}

export async function revokeSession(client: OrbitClient, id: string): Promise<void> {
  await revokeSessionRequest({ client, path: { id }, throwOnError: true })
}

/** The MCP clients this person approved. The OAuth routes are not in the OpenAPI contract, so they have no SDK function. */
export async function oauthConnections(client: OrbitClient): Promise<OAuthConnection[]> {
  return (await client.get<{ 200: { items: OAuthConnection[] } }, unknown, true>({ url: '/api/v1/oauth/grants', throwOnError: true })).data.items
}

/** Stops the client's access and refresh tokens. */
export async function disconnectOAuth(client: OrbitClient, id: string): Promise<void> {
  await client.delete({ url: '/api/v1/oauth/grants/{id}', path: { id }, throwOnError: true })
}

/** The current session first, then the most recently active. */
export function sortedSessions(list: readonly SessionRecord[]): SessionRecord[] {
  return [...list].sort((a, b) => Number(b.current) - Number(a.current) || b.last_activity_at.localeCompare(a.last_activity_at))
}

// ---- notifications

export async function notificationPrefs(client: OrbitClient): Promise<NotificationPrefs> {
  return (await getNotificationPreferences({ client, throwOnError: true })).data
}

export async function saveNotificationPrefs(client: OrbitClient, prefs: NotificationPrefs): Promise<NotificationPrefs> {
  return (await putNotificationPreferences({ client, body: prefs, throwOnError: true })).data
}

/** The web's rows, in its order. */
export const NOTIFICATION_CHOICES: readonly { field: keyof NotificationPrefs; label: string }[] = [
  { field: 'direct_messages', label: 'Direct messages' },
  { field: 'chat_mentions', label: 'Mentions in chat' },
  { field: 'thread_replies', label: 'Replies in threads I follow' },
  { field: 'channel_messages', label: 'All messages in channels set to "All messages"' },
  { field: 'task_assigned', label: 'Tasks assigned to me' },
  { field: 'mentions', label: 'Mentions in comments and pages' },
]

// ---- workspace

export async function renameWorkspace(client: OrbitClient, workspace: Pick<WorkspaceRecord, 'id' | 'version'>, name: string): Promise<WorkspaceRecord> {
  return (await renameWorkspaceRequest({ client, path: { workspace_id: workspace.id }, body: { name, expected_version: workspace.version }, throwOnError: true })).data
}

/** Admins also get one page of the largest files, from `offset`. */
export async function workspaceStorage(client: OrbitClient, workspaceId: string, offset: number): Promise<WorkspaceStorage> {
  return (await getWorkspaceStorage({ client, path: { workspace_id: workspaceId }, query: { files: STORAGE_FILES_PAGE, offset }, throwOnError: true })).data
}

/** Deletes a stored file for an admin; where it was used, it then shows as a deleted file. */
export async function deleteStorageFile(client: OrbitClient, workspaceId: string, fileId: string): Promise<void> {
  await deleteStorageFileRequest({ client, path: { workspace_id: workspaceId, file_id: fileId }, throwOnError: true })
}

/** The File storage page: the workspace's own S3 bucket for attachments. */
export async function workspaceBucket(client: OrbitClient, workspaceId: string): Promise<WorkspaceBucketView> {
  return (await getWorkspaceBucket({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data
}

/** Orbit tests the bucket before it saves; an empty secret key keeps the saved one. */
export async function saveBucket(client: OrbitClient, workspaceId: string, body: S3Body): Promise<WorkspaceBucketView> {
  return (await saveWorkspaceBucket({ client, path: { workspace_id: workspaceId }, body, throwOnError: true })).data
}

/** The bucket's files move back to Orbit's storage; the bucket goes away when none are left. */
export async function removeBucket(client: OrbitClient, workspaceId: string): Promise<WorkspaceBucketView> {
  return (await removeWorkspaceBucket({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data
}

/** Polls while files move either way, as the admin Storage page does. */
export function bucketPollInterval(view: WorkspaceBucketView | undefined): number | false {
  return view && (view.checking || view.files_to_move > 0 || view.removing) ? 5000 : false
}

/** The File storage page's progress line, or `null` when no file waits to move. */
export function bucketMovingText(view: Pick<WorkspaceBucketView, 'checking' | 'files_to_move' | 'removing'>): string | null {
  if (view.checking) return 'Checking which files to move…'
  if (view.files_to_move <= 0) return null
  return `${view.files_to_move} ${view.files_to_move === 1 ? 'file' : 'files'} left to move ${view.removing ? "back to Orbit's storage" : 'to your bucket'}.`
}

/** The web's `formatBytes`: `512 B`, `12 KB`, `1.5 MB`, `2 GB`. */
export function formatBytes(bytes: number): string {
  if (bytes >= 1_000_000_000) return `${Number((bytes / 1_000_000_000).toFixed(1))} GB`
  if (bytes >= 1_000_000) return `${Number((bytes / 1_000_000).toFixed(1))} MB`
  if (bytes >= 1000) return `${Math.round(bytes / 1000)} KB`
  return `${bytes} B`
}

/** The Storage card's line and how full the bar is (`null` without a quota). */
export function storageUse(storage: WorkspaceStorage): { label: string; fraction: number | null } {
  const quota = storage.quota_bytes ?? null
  if (!quota) return { label: `${formatBytes(storage.used_bytes)} · no limit`, fraction: null }
  return { label: `${formatBytes(storage.used_bytes)} of ${formatBytes(quota)}`, fraction: Math.min(1, storage.used_bytes / quota) }
}

// ---- members and invitations

/** What the server offers: `email_enabled` decides whether an invitation can go by email. */
export async function authOptions(client: OrbitClient): Promise<AuthOptions> {
  return (await authOptionsRequest({ client, throwOnError: true })).data
}

export const roleLabel = (role: WorkspaceRole | string) => role.charAt(0).toUpperCase() + role.slice(1)

export type MemberAction = { kind: 'role'; role: 'admin' | 'member' } | { kind: 'transfer' } | { kind: 'remove' }

/** What the Manage menu offers on a member's row: none on your own row; the server decides the rest. */
export function memberActions(member: MemberRecord, myUserId: string): MemberAction[] {
  if (member.user_id === myUserId) return []
  const roles = member.can_change_role ? (['admin', 'member'] as const).filter((role) => role !== member.role) : []
  return [
    ...roles.map((role): MemberAction => ({ kind: 'role', role })),
    ...(member.can_transfer_ownership ? [{ kind: 'transfer' } as const] : []),
    ...(member.can_remove ? [{ kind: 'remove' } as const] : []),
  ]
}

/** Members matching a search over name, email and role, by name. */
export function filterMembers(members: readonly MemberRecord[], search: string): MemberRecord[] {
  const query = search.trim().toLowerCase()
  return members
    .filter((member) => !query || [member.display_name, member.email, member.role].some((value) => value.toLowerCase().includes(query)))
    .sort((a, b) => a.display_name.localeCompare(b.display_name))
}

export async function changeRole(client: OrbitClient, workspaceId: string, member: MemberRecord, role: 'admin' | 'member'): Promise<void> {
  await changeMemberRole({ client, path: { workspace_id: workspaceId, membership_id: member.id }, body: { role, expected_version: member.version }, throwOnError: true })
}

/** Removes a member; on your own membership, leaves the workspace. */
export async function removeMembership(client: OrbitClient, workspaceId: string, member: Pick<MemberRecord, 'id' | 'version'>): Promise<void> {
  await removeMember({ client, path: { workspace_id: workspaceId, membership_id: member.id }, query: { expected_version: member.version }, throwOnError: true })
}

export async function transferOwnership(client: OrbitClient, workspace: Pick<WorkspaceRecord, 'id' | 'version'>, member: MemberRecord): Promise<void> {
  await transferOwnershipRequest({
    client,
    path: { workspace_id: workspace.id },
    body: { membership_id: member.id, membership_version: member.version, expected_version: workspace.version },
    throwOnError: true,
  })
}

export async function pendingInvitations(client: OrbitClient, workspaceId: string): Promise<InvitationRecord[]> {
  const { data } = await listInvitations({ client, path: { workspace_id: workspaceId }, throwOnError: true })
  return data.items.filter((invitation) => invitation.status === 'pending')
}

/** `manual` answers with the link to share; `smtp` emails it. */
export async function invite(client: OrbitClient, workspaceId: string, email: string, role: 'admin' | 'member', delivery: 'manual' | 'smtp'): Promise<{ invitation: InvitationRecord; url: string | null }> {
  const { data } = await createInvitation({ client, path: { workspace_id: workspaceId }, body: { email, role, delivery }, throwOnError: true })
  return { invitation: data.invitation, url: data.url ?? null }
}

export async function revokeInvitation(client: OrbitClient, workspaceId: string, invitationId: string): Promise<void> {
  await revokeInvitationRequest({ client, path: { workspace_id: workspaceId, invitation_id: invitationId }, throwOnError: true })
}
