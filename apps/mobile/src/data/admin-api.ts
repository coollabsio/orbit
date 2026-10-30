/**
 * The installation Admin calls the app offers (instance settings, storage, users, audit log, backups, background job
 * limits) over the generated SDK, with the rules of the web's Admin forms. No React. The server refuses every admin
 * request from other accounts.
 */
import {
  changeUserEmail,
  createBackup as createBackupRequest,
  createRecoveryLink,
  deleteBackup as deleteBackupRequest,
  getInstanceSettings,
  getJobLimits,
  listAdminUsers,
  listBackups,
  listGlobalAudit,
  removeS3 as removeS3Request,
  removeSmtp as removeSmtpRequest,
  resetTwoFactor,
  saveJobLimits as saveJobLimitsRequest,
  saveLimits as saveLimitsRequest,
  saveS3 as saveS3Request,
  saveSmtp as saveSmtpRequest,
  saveStorageOptions as saveStorageOptionsRequest,
  sendTestEmail as sendTestEmailRequest,
  setAccountSuspension,
  setInstanceAdmin,
  setRegistration as setRegistrationRequest,
  startImageCompression as startImageCompressionRequest,
} from '@orbit/api/generated/sdk.gen'
import type {
  AdminUser,
  AuthOptions,
  BackupCreated,
  BackupSchedule,
  BackupSummary,
  CompressionStatus,
  InstanceSettingsView,
  JobLimitsBody,
  JobLimitView,
  LimitsView,
  PageAuditEvent,
  RecoveryLink,
  S3Body,
  S3View,
  SmtpBody,
  SmtpSecurity,
  SmtpView,
  StorageOptionsBody,
  StorageView,
} from '@orbit/api/generated/types.gen'

import { formatBytes } from './account-api'
import type { OrbitClient } from './orbit'

export type { AdminUser, BackupSchedule, BackupSummary, CompressionStatus, InstanceSettingsView, JobLimitView, LimitsView, RecoveryLink, S3View, SmtpSecurity, SmtpView, StorageView }
export type AuditEvent = PageAuditEvent['items'][number]

export interface Page<T> {
  items: T[]
  next: string | null
}

export const adminKeys = {
  users: (search: string) => ['admin', 'users', search] as const,
  audit: ['admin', 'audit'] as const,
  backups: ['admin', 'backups'] as const,
  jobs: ['admin', 'jobs'] as const,
  settings: ['admin', 'settings'] as const,
}

/** The audit log's path, for the CSV export. */
export const AUDIT_EXPORT_PATH = '/api/v1/admin/audit/export'

export async function adminUsers(client: OrbitClient, search: string, cursor?: string): Promise<Page<AdminUser>> {
  const { data } = await listAdminUsers({ client, query: { q: search || undefined, cursor, limit: 50 }, throwOnError: true })
  return { items: data.items, next: data.next_cursor ?? null }
}

export async function auditEvents(client: OrbitClient, cursor?: string): Promise<Page<AuditEvent>> {
  const { data } = await listGlobalAudit({ client, query: { cursor, limit: 50 }, throwOnError: true })
  return { items: data.items, next: data.next_cursor ?? null }
}

export async function backups(client: OrbitClient): Promise<BackupSummary[]> {
  return (await listBackups({ client, throwOnError: true })).data.items
}

export async function jobLimits(client: OrbitClient): Promise<JobLimitView[]> {
  return (await getJobLimits({ client, throwOnError: true })).data.kinds
}

export async function saveJobLimits(client: OrbitClient, body: JobLimitsBody): Promise<JobLimitView[]> {
  return (await saveJobLimitsRequest({ client, body, throwOnError: true })).data.kinds
}

export async function createBackup(client: OrbitClient): Promise<BackupCreated> {
  return (await createBackupRequest({ client, throwOnError: true })).data
}

export async function deleteBackup(client: OrbitClient, backupId: string): Promise<void> {
  await deleteBackupRequest({ client, path: { backup_id: backupId }, throwOnError: true })
}

export async function instanceSettings(client: OrbitClient): Promise<InstanceSettingsView> {
  return (await getInstanceSettings({ client, throwOnError: true })).data
}

// Each settings write answers with the whole settings.
export async function setRegistration(client: OrbitClient, open: boolean): Promise<InstanceSettingsView> {
  return (await setRegistrationRequest({ client, body: { open }, throwOnError: true })).data
}

export async function saveSmtp(client: OrbitClient, body: SmtpBody): Promise<InstanceSettingsView> {
  return (await saveSmtpRequest({ client, body, throwOnError: true })).data
}

export async function removeSmtp(client: OrbitClient): Promise<InstanceSettingsView> {
  return (await removeSmtpRequest({ client, throwOnError: true })).data
}

export async function sendTestEmail(client: OrbitClient): Promise<void> {
  await sendTestEmailRequest({ client, throwOnError: true })
}

export async function saveLimits(client: OrbitClient, body: LimitsView): Promise<InstanceSettingsView> {
  return (await saveLimitsRequest({ client, body, throwOnError: true })).data
}

export async function saveS3(client: OrbitClient, body: S3Body): Promise<InstanceSettingsView> {
  return (await saveS3Request({ client, body, throwOnError: true })).data
}

export async function removeS3(client: OrbitClient): Promise<InstanceSettingsView> {
  return (await removeS3Request({ client, throwOnError: true })).data
}

export async function saveStorageOptions(client: OrbitClient, body: StorageOptionsBody): Promise<InstanceSettingsView> {
  return (await saveStorageOptionsRequest({ client, body, throwOnError: true })).data
}

export async function startImageCompression(client: OrbitClient): Promise<InstanceSettingsView> {
  return (await startImageCompressionRequest({ client, throwOnError: true })).data
}

export async function setSuspended(client: OrbitClient, userId: string, suspended: boolean): Promise<void> {
  await setAccountSuspension({ client, path: { user_id: userId }, body: { suspended }, throwOnError: true })
}

export async function setAdmin(client: OrbitClient, userId: string, admin: boolean): Promise<void> {
  await setInstanceAdmin({ client, path: { user_id: userId }, body: { admin }, throwOnError: true })
}

export async function recoveryLink(client: OrbitClient, userId: string): Promise<RecoveryLink> {
  return (await createRecoveryLink({ client, path: { user_id: userId }, throwOnError: true })).data
}

export async function clearTwoFactor(client: OrbitClient, userId: string): Promise<void> {
  await resetTwoFactor({ client, path: { user_id: userId }, throwOnError: true })
}

/** Changes an account's email at once, with no confirmation email; the account is signed out everywhere. */
export async function changeEmail(client: OrbitClient, userId: string, email: string): Promise<void> {
  await changeUserEmail({ client, path: { user_id: userId }, body: { email: email.trim() }, throwOnError: true })
}

export type AdminAction = 'change-email' | 'make-admin' | 'remove-admin' | 'recovery-link' | 'reset-two-factor' | 'suspend' | 'reinstate'

/**
 * The web's Manage menu: nobody manages the root account, their own or a deleted one, except that an admin may
 * change their own email; only the root user manages instance admins. A suspended account can only be reinstated
 * (or get another email).
 */
export function adminActions(user: AdminUser, viewer: { id: string; root: boolean }): AdminAction[] {
  if (user.deleted) return []
  if (user.id === viewer.id) return ['change-email']
  if (user.root || (user.admin && !viewer.root)) return []
  const admin: AdminAction[] = viewer.root ? (user.admin ? ['remove-admin'] : user.suspended ? [] : ['make-admin']) : []
  if (user.suspended) return ['change-email', ...admin, 'reinstate']
  return ['change-email', ...admin, 'recovery-link', ...(user.two_factor ? (['reset-two-factor'] as const) : []), 'suspend']
}

/** Orbit Cloud emails the person about these, so they need a mail server there. */
const EMAILS_THE_PERSON_ON_CLOUD: ReadonlySet<AdminAction> = new Set(['change-email', 'recovery-link', 'reset-two-factor'])

/** Why the Manage menu offers `action` but turns it off, or null when it can run. */
export function adminActionBlocked(action: AdminAction, options: Pick<AuthOptions, 'billing_enabled' | 'email_enabled'> | undefined): string | null {
  return options?.billing_enabled === true && !options.email_enabled && EMAILS_THE_PERSON_ON_CLOUD.has(action) ? 'Needs a mail server on Orbit Cloud' : null
}

/** `Root`, `Admin`, `2FA`, `Suspended`: the badges beside a user's name. A deleted account has only `Deleted`. */
export function adminBadges(user: AdminUser): string[] {
  if (user.deleted) return ['Deleted']
  return [user.root ? 'Root' : user.admin ? 'Admin' : null, user.two_factor ? '2FA' : null, user.suspended ? 'Suspended' : null].filter((badge): badge is string => badge !== null)
}

/** Who did it: the account's name, or `System` for the server's own events. */
export const auditActor = (event: Pick<AuditEvent, 'actor_id' | 'actor_name'>) => (event.actor_id ? (event.actor_name ?? 'Unknown account') : 'System')

/** The web's job names; an unknown kind shows as it is. */
const JOB_LABELS: Record<string, string> = { 'notion.scan': 'Notion scans', 'notion.import': 'Notion imports' }
export const jobLabel = (kind: string) => JOB_LABELS[kind] ?? kind
export const limitText = (limit: number | null | undefined) => (limit == null ? 'no limit' : String(limit))

/** How often the settings refresh: every 2 s while images compress, every 5 s while files move, else not at all. */
export function settingsPollInterval(storage: StorageView | undefined): number | false {
  if (storage?.image_compression?.running) return 2000
  return storage?.files_to_move ? 5000 : false
}

export const SMTP_SECURITY_OPTIONS: readonly { value: SmtpSecurity; label: string }[] = [
  { value: 'starttls', label: 'STARTTLS' },
  { value: 'tls', label: 'TLS' },
  { value: 'none', label: 'None' },
]
/** The port each encryption uses; choosing an encryption puts its port in the field. */
export const SMTP_DEFAULT_PORT: Record<SmtpSecurity, number> = { starttls: 587, tls: 465, none: 25 }

export interface SmtpDraft {
  host: string
  port: string
  security: SmtpSecurity
  username: string
  password: string
  from_address: string
  from_name: string
}

/** The Email form for the saved server; the password field starts empty (the server never returns it). */
export function smtpDraft(smtp: SmtpView | null): SmtpDraft {
  return {
    host: smtp?.host ?? '',
    port: String(smtp?.port ?? SMTP_DEFAULT_PORT.starttls),
    security: smtp?.security ?? 'starttls',
    username: smtp?.username ?? '',
    password: '',
    from_address: smtp?.from_address ?? '',
    from_name: smtp?.from_name ?? '',
  }
}

/** What the Email form saves. An empty password keeps the saved one. */
export function smtpBody(draft: SmtpDraft): SmtpBody {
  return {
    host: draft.host.trim(),
    port: Number(draft.port),
    security: draft.security,
    username: draft.username.trim() || null,
    password: draft.password || undefined,
    from_address: draft.from_address.trim(),
    from_name: draft.from_name.trim() || null,
  }
}

export const BYTES_PER_GB = 1_000_000_000

/** A whole number above 0, or `null` (no limit) for an empty field or 0. `undefined` when it is not a number. */
export function parseLimit(value: string, scale = 1): number | null | undefined {
  if (!value.trim()) return null
  const number = Number(value)
  if (!Number.isFinite(number) || number < 0) return undefined
  return number === 0 ? null : Math.round(number * scale)
}

/** The Limits form's fields: workspaces per account, and storage per workspace in GB. */
export function limitsDraft(limits: LimitsView): { workspaces: string; storageGb: string } {
  return {
    workspaces: limits.max_owned_workspaces ? String(limits.max_owned_workspaces) : '',
    storageGb: limits.workspace_storage_quota_bytes ? String(limits.workspace_storage_quota_bytes / BYTES_PER_GB) : '',
  }
}

/** What the Limits form saves, or `null` when a field is not a number. */
export function limitsBody(draft: { workspaces: string; storageGb: string }): LimitsView | null {
  const workspaces = parseLimit(draft.workspaces)
  const storage = parseLimit(draft.storageGb, BYTES_PER_GB)
  if (workspaces === undefined || storage === undefined) return null
  return { max_owned_workspaces: workspaces, workspace_storage_quota_bytes: storage }
}

export interface S3Draft {
  endpoint: string
  region: string
  bucket: string
  prefix: string
  access_key_id: string
  secret_access_key: string
  path_style: boolean
}

/** The S3 bucket form for the saved bucket; the secret key field starts empty (the server never returns it). */
export function s3Draft(s3: S3View | null): S3Draft {
  return {
    endpoint: s3?.endpoint ?? '',
    region: s3?.region ?? '',
    bucket: s3?.bucket ?? '',
    prefix: s3?.prefix ?? '',
    access_key_id: s3?.access_key_id ?? '',
    secret_access_key: '',
    path_style: s3?.path_style ?? false,
  }
}

/** What the bucket form saves. An empty secret key keeps the saved one. */
export function s3Body(draft: S3Draft): S3Body {
  return {
    endpoint: draft.endpoint.trim(),
    region: draft.region.trim() || null,
    bucket: draft.bucket.trim(),
    prefix: draft.prefix.trim() || null,
    access_key_id: draft.access_key_id.trim(),
    secret_access_key: draft.secret_access_key || undefined,
    path_style: draft.path_style,
  }
}

export const BACKUP_SCHEDULE_OPTIONS: readonly { value: BackupSchedule; label: string }[] = [
  { value: 'off', label: 'Off' },
  { value: 'hourly', label: 'Every hour' },
  { value: 'daily', label: 'Every day' },
]

/** The Storage page's options with one change, as the server wants all three. */
export function storageOptions(storage: StorageView, change: Partial<StorageOptionsBody>): StorageOptionsBody {
  return { attachments_in_s3: storage.attachments_in_s3, backups_in_s3: storage.backups_in_s3, backup_schedule: storage.backup_schedule, ...change }
}

/** `3 files left to move to the bucket.`, or `null` when nothing is waiting. */
export function movingText(storage: Pick<StorageView, 'files_to_move' | 'attachments_in_s3'>): string | null {
  if (storage.files_to_move <= 0) return null
  return `${storage.files_to_move} ${storage.files_to_move === 1 ? 'file' : 'files'} left to move ${storage.attachments_in_s3 ? 'to the bucket' : 'to this server'}.`
}

/** The Images card's status line, or `null` before the first run. */
export function compressionText(status: CompressionStatus): string | null {
  const done = status.compressed + status.skipped
  const summary = status.compressed > 0
    ? `${status.compressed} ${status.compressed === 1 ? 'image' : 'images'} compressed: ${formatBytes(status.bytes_before)} → ${formatBytes(status.bytes_after)}.`
    : 'No image was compressed.'
  if (status.running) return `Compressing… ${done} ${done === 1 ? 'file' : 'files'} checked. ${summary}`
  return done > 0 ? `Done. ${summary}` : null
}

export type JobDraft = Record<string, { concurrency: string; workspace_concurrency: string }>

const limitField = (limit: number | null | undefined) => (limit == null ? '' : String(limit))

/** The Background jobs form: each kind's two limits, or their defaults (`Reset to defaults`). */
export function jobDraft(kinds: readonly JobLimitView[], useDefaults = false): JobDraft {
  return Object.fromEntries(kinds.map((kind) => [kind.kind, {
    concurrency: limitField(useDefaults ? kind.default_concurrency : kind.concurrency),
    workspace_concurrency: limitField(useDefaults ? kind.default_workspace_concurrency : kind.workspace_concurrency),
  }]))
}

/** What the Background jobs form saves; an empty field is no limit. */
export function jobLimitsBody(kinds: readonly JobLimitView[], draft: JobDraft): JobLimitsBody {
  const limit = (value: string) => (value.trim() === '' ? null : Number(value))
  return {
    kinds: kinds.map((kind) => ({
      kind: kind.kind,
      concurrency: limit(draft[kind.kind]?.concurrency ?? ''),
      workspace_concurrency: limit(draft[kind.kind]?.workspace_concurrency ?? ''),
    })),
  }
}
