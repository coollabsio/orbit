import { expect, test } from 'bun:test'

import {
  adminActionBlocked,
  adminActions,
  adminBadges,
  type AdminUser,
  auditActor,
  compressionText,
  jobDraft,
  jobLabel,
  jobLimitsBody,
  limitsBody,
  limitsDraft,
  limitText,
  movingText,
  parseLimit,
  s3Body,
  s3Draft,
  settingsPollInterval,
  smtpBody,
  smtpDraft,
  type StorageView,
  storageOptions,
} from './admin-api'

const user = (fields: Partial<AdminUser>): AdminUser => ({
  id: 'u1',
  display_name: 'Ada',
  email: 'ada@example.com',
  admin: false,
  root: false,
  suspended: false,
  deleted: false,
  two_factor: false,
  workspace_count: 1,
  created_at: '2026-10-01T10:00:00Z',
  ...fields,
})
const root = { id: 'root', root: true }
const admin = { id: 'admin', root: false }

test('nobody manages the root account, their own (but its email), or (unless root) another admin', () => {
  expect(adminActions(user({ root: true }), root)).toEqual([])
  expect(adminActions(user({ id: 'admin', admin: true }), admin)).toEqual(['change-email'])
  expect(adminActions(user({ id: 'root', root: true, admin: true }), { id: 'root', root: true })).toEqual(['change-email'])
  expect(adminActions(user({ admin: true }), admin)).toEqual([])
})

test('a deleted account has only the Deleted badge and no actions, not even reinstate', () => {
  const deleted = user({ deleted: true, suspended: true, two_factor: true })
  expect(adminActions(deleted, root)).toEqual([])
  expect(adminBadges(deleted)).toEqual(['Deleted'])
})

test('the root user manages admins; a suspended account can only come back', () => {
  expect(adminActions(user({}), root)).toEqual(['change-email', 'make-admin', 'recovery-link', 'suspend'])
  expect(adminActions(user({ admin: true, two_factor: true }), root)).toEqual(['change-email', 'remove-admin', 'recovery-link', 'reset-two-factor', 'suspend'])
  expect(adminActions(user({ suspended: true }), root)).toEqual(['change-email', 'reinstate'])
  expect(adminActions(user({}), admin)).toEqual(['change-email', 'recovery-link', 'suspend'])
})

test('badges, the audit actor and job wording', () => {
  expect(adminBadges(user({ root: true, admin: true, two_factor: true }))).toEqual(['Root', '2FA'])
  expect(adminBadges(user({ admin: true, suspended: true }))).toEqual(['Admin', 'Suspended'])
  expect(auditActor({ actor_id: null, actor_name: null })).toBe('System')
  expect(auditActor({ actor_id: 'u', actor_name: null })).toBe('Unknown account')
  expect(auditActor({ actor_id: 'u', actor_name: 'Ada' })).toBe('Ada')
  expect([jobLabel('notion.scan'), jobLabel('mail.sync'), limitText(null), limitText(4)]).toEqual(['Notion scans', 'mail.sync', 'no limit', '4'])
})

const storage = (fields: Partial<StorageView>): StorageView => ({ attachments_in_s3: false, backups_in_s3: false, backup_schedule: 'off', files_to_move: 0, ...fields })

test('the settings poll while images compress or files move', () => {
  expect(settingsPollInterval(undefined)).toBe(false)
  expect(settingsPollInterval(storage({}))).toBe(false)
  expect(settingsPollInterval(storage({ files_to_move: 3 }))).toBe(5000)
  expect(settingsPollInterval(storage({ files_to_move: 3, image_compression: { running: true, compressed: 0, skipped: 0, bytes_before: 0, bytes_after: 0 } }))).toBe(2000)
})

test('the Email form keeps the saved password when the field is empty and trims the rest', () => {
  const draft = smtpDraft(null)
  expect(draft).toEqual({ host: '', port: '587', security: 'starttls', username: '', password: '', from_address: '', from_name: '' })
  expect(smtpBody({ ...draft, host: ' smtp.example.com ', from_address: ' orbit@example.com', port: '465', security: 'tls' })).toEqual({
    host: 'smtp.example.com',
    port: 465,
    security: 'tls',
    username: null,
    password: undefined,
    from_address: 'orbit@example.com',
    from_name: null,
  })
  const saved = smtpDraft({ host: 'h', port: 25, security: 'none', username: 'u', password_set: true, from_address: 'a@b.c', from_name: 'Orbit' })
  expect(smtpBody({ ...saved, password: 'secret' })).toMatchObject({ username: 'u', password: 'secret', from_name: 'Orbit', port: 25 })
})

test('limits: empty or 0 is no limit, storage is in GB, anything else is refused', () => {
  expect([parseLimit(''), parseLimit('0'), parseLimit('3'), parseLimit('1.5', 1_000_000_000), parseLimit('-1'), parseLimit('x')]).toEqual([null, null, 3, 1_500_000_000, undefined, undefined])
  expect(limitsDraft({ max_owned_workspaces: 2, workspace_storage_quota_bytes: 2_500_000_000 })).toEqual({ workspaces: '2', storageGb: '2.5' })
  expect(limitsDraft({})).toEqual({ workspaces: '', storageGb: '' })
  expect(limitsBody({ workspaces: '', storageGb: '10' })).toEqual({ max_owned_workspaces: null, workspace_storage_quota_bytes: 10_000_000_000 })
  expect(limitsBody({ workspaces: 'many', storageGb: '' })).toBeNull()
})

test('the bucket form keeps the saved secret key when the field is empty', () => {
  const draft = s3Draft({ endpoint: 'https://s3', region: '', bucket: 'b', prefix: '', access_key_id: 'k', path_style: true })
  expect(s3Body(draft)).toEqual({ endpoint: 'https://s3', region: null, bucket: 'b', prefix: null, access_key_id: 'k', secret_access_key: undefined, path_style: true })
  expect(s3Body({ ...draft, secret_access_key: 's', prefix: ' orbit ' })).toMatchObject({ secret_access_key: 's', prefix: 'orbit' })
})

test('storage options change one field and keep the others', () => {
  expect(storageOptions(storage({ backups_in_s3: true, backup_schedule: 'daily' }), { attachments_in_s3: true })).toEqual({ attachments_in_s3: true, backups_in_s3: true, backup_schedule: 'daily' })
})

test('moving and compression wording', () => {
  expect(movingText({ files_to_move: 0, attachments_in_s3: true })).toBeNull()
  expect(movingText({ files_to_move: 1, attachments_in_s3: true })).toBe('1 file left to move to the bucket.')
  expect(movingText({ files_to_move: 4, attachments_in_s3: false })).toBe('4 files left to move to this server.')
  expect(compressionText({ running: false, compressed: 0, skipped: 0, bytes_before: 0, bytes_after: 0 })).toBeNull()
  expect(compressionText({ running: true, compressed: 0, skipped: 1, bytes_before: 0, bytes_after: 0 })).toBe('Compressing… 1 file checked. No image was compressed.')
  expect(compressionText({ running: false, compressed: 2, skipped: 1, bytes_before: 3_000_000, bytes_after: 1_000_000 })).toBe('Done. 2 images compressed: 3 MB → 1 MB.')
})

test('job limits: the form, its defaults, and what it saves', () => {
  const kinds = [{ kind: 'notion.import', concurrency: 2, workspace_concurrency: null, default_concurrency: 4, default_workspace_concurrency: 1 }]
  expect(jobDraft(kinds)).toEqual({ 'notion.import': { concurrency: '2', workspace_concurrency: '' } })
  expect(jobDraft(kinds, true)).toEqual({ 'notion.import': { concurrency: '4', workspace_concurrency: '1' } })
  expect(jobLimitsBody(kinds, { 'notion.import': { concurrency: ' ', workspace_concurrency: '3' } })).toEqual({ kinds: [{ kind: 'notion.import', concurrency: null, workspace_concurrency: 3 }] })
})

test('Orbit Cloud without a mail server turns off the actions that email the person, and says why', () => {
  const actions = adminActions(user({ two_factor: true }), root)
  const blocked = (options: { billing_enabled: boolean; email_enabled: boolean } | undefined) => actions.map((action) => [action, adminActionBlocked(action, options)])
  expect(blocked({ billing_enabled: true, email_enabled: false })).toEqual([
    ['change-email', 'Needs a mail server on Orbit Cloud'],
    ['make-admin', null],
    ['recovery-link', 'Needs a mail server on Orbit Cloud'],
    ['reset-two-factor', 'Needs a mail server on Orbit Cloud'],
    ['suspend', null],
  ])
  expect(blocked({ billing_enabled: true, email_enabled: true }).filter(([, reason]) => reason !== null)).toEqual([])
  expect(blocked({ billing_enabled: false, email_enabled: false }).filter(([, reason]) => reason !== null)).toEqual([])
  expect(blocked(undefined).filter(([, reason]) => reason !== null)).toEqual([])
})
