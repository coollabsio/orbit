import { createOrbitClient } from '@orbit/api/client'
import { expect, test } from 'bun:test'

import { bucketMovedText, bucketMovingText, bucketPollInterval, confirmsEmail, createWorkspaceRole, deleteAccount, filterMembers, formatBytes, invitationProblem, memberActions, ownedWorkspaces, passwordProblem, pickedImageName, requestEmailChange, roleSummary, setCustomRole, sortedSessions, storageUse, twoFactorSummary, updateWorkspaceRole } from './account-api'
import type { MemberRecord } from './orbit'

const member = (id: string, fields: Partial<MemberRecord>): MemberRecord => ({
  id: `m-${id}`,
  user_id: id,
  display_name: id,
  email: `${id}@example.com`,
  role: 'member',
  can_change_role: false,
  can_remove: false,
  can_transfer_ownership: false,
  created_at: '2026-10-01T10:00:00Z',
  version: 1,
  ...fields,
})

test('the Manage menu: no actions on your own row, the other role, transfer, then remove', () => {
  const ada = member('ada', { role: 'admin', can_change_role: true, can_remove: true, can_transfer_ownership: true })
  expect(memberActions(ada, 'me')).toEqual([{ kind: 'role', role: 'member' }, { kind: 'transfer' }, { kind: 'remove' }])
  expect(memberActions({ ...ada, user_id: 'me' }, 'me')).toEqual([])
  expect(memberActions(member('bob', { can_change_role: true }), 'me')).toEqual([{ kind: 'role', role: 'admin' }])
  expect(memberActions(member('eve', {}), 'me')).toEqual([])
})

test('members are searched by name, email and role, and listed by name', () => {
  const list = [member('zoe', { role: 'owner' }), member('ada', {}), member('bob', { email: 'robert@corp.io' })]
  expect(filterMembers(list, '').map((m) => m.user_id)).toEqual(['ada', 'bob', 'zoe'])
  expect(filterMembers(list, 'OWNER').map((m) => m.user_id)).toEqual(['zoe'])
  expect(filterMembers(list, 'corp').map((m) => m.user_id)).toEqual(['bob'])
})

test('bytes read as the web writes them', () => {
  expect([formatBytes(512), formatBytes(12_345), formatBytes(1_500_000), formatBytes(2_000_000_000)]).toEqual(['512 B', '12 KB', '1.5 MB', '2 GB'])
  expect(storageUse({ used_bytes: 250_000_000, quota_bytes: 1_000_000_000 })).toEqual({ label: '250 MB of 1 GB', fraction: 0.25 })
  expect(storageUse({ used_bytes: 3000, quota_bytes: null })).toEqual({ label: '3 KB · no limit', fraction: null })
})

test('a password change needs all three fields and a matching confirmation', () => {
  expect(passwordProblem('old', 'new', '')).toBe('Fill in all three fields.')
  expect(passwordProblem('old', 'new1', 'new2')).toBe('The new password confirmation does not match.')
  expect(passwordProblem('old', 'new', 'new')).toBeNull()
})

test('two-factor status in words', () => {
  expect(twoFactorSummary({ totp_enabled: false, recovery_codes_left: 0 })).toBe('Only your password is needed to sign in.')
  expect(twoFactorSummary({ totp_enabled: true, recovery_codes_left: 1 })).toBe('On · 1 recovery code left')
  expect(twoFactorSummary({ totp_enabled: true, recovery_codes_left: 8 })).toBe('On · 8 recovery codes left')
})

test('sessions: this one first, then the most recently active', () => {
  const session = (id: string, current: boolean, last: string) => ({ id, current, last_activity_at: last, created_at: last, idle_expires_at: last, absolute_expires_at: last, user: { id: 'u', email: 'u@x', display_name: 'U' } })
  const sorted = sortedSessions([session('a', false, '2026-10-01'), session('b', true, '2026-09-01'), session('c', false, '2026-10-05')])
  expect(sorted.map((item) => item.id)).toEqual(['b', 'c', 'a'])
})

test('a picked photo is uploaded under its file name, else one from its type', () => {
  expect(pickedImageName({ uri: 'file:///tmp/a.png' })).toBe('picture.png')
  expect(pickedImageName({ uri: 'file:///tmp/b', fileName: 'IMG_1.HEIC', mimeType: 'image/jpeg' })).toBe('IMG_1.HEIC')
  expect(pickedImageName({ uri: 'file:///tmp/c' })).toBe('picture.jpg')
})

test('bucketPollInterval polls while files move either way', () => {
  expect(bucketPollInterval(undefined)).toBe(false)
  expect(bucketPollInterval({ s3: null, removing: false, checking: false, files_to_move: 0 })).toBe(false)
  expect(bucketPollInterval({ s3: null, removing: false, checking: false, files_to_move: 3 })).toBe(5000)
  expect(bucketPollInterval({ s3: null, removing: true, checking: false, files_to_move: 0 })).toBe(5000)
  expect(bucketPollInterval({ s3: null, removing: false, checking: true, files_to_move: 0 })).toBe(1000)
})

test('bucketMovingText names the direction', () => {
  expect(bucketMovingText({ removing: false, checking: false, files_to_move: 0 })).toBeNull()
  expect(bucketMovingText({ removing: false, checking: false, files_to_move: 1 })).toBe('1 file left to move to your bucket.')
  expect(bucketMovingText({ removing: true, checking: false, files_to_move: 12 })).toBe("12 files left to move back to Orbit's storage.")
  expect(bucketMovingText({ removing: false, checking: true, files_to_move: 0 })).toBe('Checking which files to move…')
})

test('bucketMovedText says done only after a move the admin started', () => {
  const idle = { removing: false, checking: false, files_to_move: 0, move_error: null }
  expect(bucketMovedText(idle, null)).toBeNull()
  expect(bucketMovedText({ ...idle, checking: true }, 'save')).toBeNull()
  expect(bucketMovedText({ ...idle, removing: true }, 'remove')).toBeNull()
  expect(bucketMovedText({ ...idle, move_error: 'denied' }, 'save')).toBeNull()
  expect(bucketMovedText(idle, 'save')).toBe('Done. All files are in your bucket.')
  expect(bucketMovedText(idle, 'remove')).toBe("Done. All files are back in Orbit's storage.")
})

test('a guest only becomes a member, and nobody becomes a guest', () => {
  const guest = { user_id: 'gus', role: 'guest', can_change_role: true, can_remove: true, can_transfer_ownership: false } as unknown as Parameters<typeof memberActions>[0]
  expect(memberActions(guest, 'me')).toEqual([{ kind: 'role', role: 'member' }, { kind: 'remove' }])
  const member = { ...guest, role: 'member' } as typeof guest
  expect(memberActions(member, 'me')).toEqual([{ kind: 'role', role: 'admin' }, { kind: 'remove' }])
})

test('a guest invitation needs one or more channels', () => {
  expect(invitationProblem('guest', [])).toBe('Choose one or more channels for the guest.')
  expect(invitationProblem('guest', ['c1'])).toBeNull()
  expect(invitationProblem('member', [])).toBeNull()
})

/** A client whose fetch records each request and answers with `reply`. */
function server(reply: { status: number; body?: unknown }) {
  const sent: { method: string; url: string; body: unknown }[] = []
  const client = createOrbitClient({
    baseUrl: 'https://orbit.test',
    token: () => 'session-token',
    fetch: async (request) => {
      sent.push({ method: request.method, url: request.url, body: JSON.parse(await request.text()) })
      return reply.body === undefined
        ? new Response(null, { status: reply.status })
        : new Response(JSON.stringify(reply.body), { status: reply.status, headers: { 'content-type': 'application/problem+json' } })
    },
  })
  return { client, sent }
}

test('an email change sends the new address, and the password only when there is one', async () => {
  const withPassword = server({ status: 202, body: { detail: 'We sent a link.' } })
  await requestEmailChange(withPassword.client, 'new@orbit.test', 'secret')
  expect(withPassword.sent).toEqual([{ method: 'POST', url: 'https://orbit.test/api/v1/auth/me/email', body: { email: 'new@orbit.test', password: 'secret' } }])
  const without = server({ status: 202, body: { detail: 'We sent a link.' } })
  await requestEmailChange(without.client, 'new@orbit.test')
  expect(without.sent[0].body).toEqual({ email: 'new@orbit.test' })
})

const problem = (status: number, code: string, extra: object = {}) => ({ type: 'about:blank', title: 'Refused', status, code, detail: `The ${code} detail.`, instance: '/api/v1/auth/me', request_id: 'r', ...extra })

test('deleting the account sends the typed email and the password; owned workspaces come back from the 409', async () => {
  const ok = server({ status: 204 })
  await deleteAccount(ok.client, 'me@orbit.test', 'secret')
  expect(ok.sent).toEqual([{ method: 'DELETE', url: 'https://orbit.test/api/v1/auth/me', body: { email: 'me@orbit.test', password: 'secret' } }])

  const owns = server({ status: 409, body: problem(409, 'owns_workspaces', { workspaces: [{ id: 'w1', name: 'Alpha' }, { id: 'w2', name: 'Beta' }] }) })
  const error = await deleteAccount(owns.client, 'me@orbit.test', 'secret').catch((reason: unknown) => reason)
  expect(ownedWorkspaces(error)).toEqual([{ id: 'w1', name: 'Alpha' }, { id: 'w2', name: 'Beta' }])

  const root = server({ status: 409, body: problem(409, 'root_account') })
  const rootError = await deleteAccount(root.client, 'me@orbit.test', 'secret').catch((reason: unknown) => reason)
  expect(ownedWorkspaces(rootError)).toBeNull()
  expect((rootError as Error).message).toBe('The root_account detail.')
})

test('the typed confirmation matches the email regardless of case and spaces', () => {
  expect(confirmsEmail(' Me@Orbit.test ', 'me@orbit.test')).toBe(true)
  expect(confirmsEmail('me@orbit.tes', 'me@orbit.test')).toBe(false)
})

test('only someone who manages roles may give an ordinary member a custom role', () => {
  const bob = member('bob', { can_change_role: true, can_remove: true })
  expect(memberActions(bob, 'me', true)).toEqual([{ kind: 'role', role: 'admin' }, { kind: 'customRole' }, { kind: 'remove' }])
  expect(memberActions(bob, 'me')).toEqual([{ kind: 'role', role: 'admin' }, { kind: 'remove' }])
  expect(memberActions(member('ada', { role: 'admin' }), 'me', true)).toEqual([])
  expect(memberActions(member('gus', { role: 'guest' }), 'me', true)).toEqual([])
})

test('members are found by their custom role', () => {
  const list = [member('ada', { custom_role: { id: 'r1', name: 'Moderator' } }), member('bob', { custom_role: null }), member('eve', {})]
  expect(filterMembers(list, 'moder').map((m) => m.user_id)).toEqual(['ada'])
})

test('a role reads as its member and permission counts', () => {
  expect(roleSummary({ member_count: 1, permissions: ['chat.manage'] })).toBe('1 member · 1 permission')
  expect(roleSummary({ member_count: 3, permissions: [] })).toBe('3 members · no extra permissions')
  expect(roleSummary({ member_count: 0, permissions: ['chat.manage', 'audit.view'] })).toBe('0 members · 2 permissions')
})

const role = { id: 'r1', name: 'Moderator', permissions: ['chat.manage' as const], member_count: 2, version: 4, created_at: '2026-11-12T10:00:00Z', updated_at: '2026-11-12T10:00:00Z' }

test('a role is created and edited with its trimmed name, its permissions and, on an edit, its version', async () => {
  const created = server({ status: 201, body: role })
  expect(await createWorkspaceRole(created.client, 'w1', { name: '  Moderator ', permissions: ['chat.manage'] })).toEqual(role)
  expect(created.sent).toEqual([{ method: 'POST', url: 'https://orbit.test/api/v1/workspaces/w1/roles', body: { name: 'Moderator', permissions: ['chat.manage'] } }])
  const edited = server({ status: 200, body: role })
  await updateWorkspaceRole(edited.client, 'w1', role, { name: 'Moderator', permissions: [] })
  expect(edited.sent).toEqual([{ method: 'PATCH', url: 'https://orbit.test/api/v1/workspaces/w1/roles/r1', body: { expected_version: 4, name: 'Moderator', permissions: [] } }])
})

test('a custom role is given and taken away with the membership version; a refusal carries the server detail', async () => {
  const given = server({ status: 204 })
  await setCustomRole(given.client, 'w1', { id: 'm-bob', version: 7 }, 'r1')
  await setCustomRole(given.client, 'w1', { id: 'm-bob', version: 8 }, null)
  expect(given.sent).toEqual([
    { method: 'PUT', url: 'https://orbit.test/api/v1/workspaces/w1/members/m-bob/custom-role', body: { role_id: 'r1', expected_version: 7 } },
    { method: 'PUT', url: 'https://orbit.test/api/v1/workspaces/w1/members/m-bob/custom-role', body: { role_id: null, expected_version: 8 } },
  ])
  const refused = server({ status: 422, body: problem(422, 'invalid_role') })
  await expect(setCustomRole(refused.client, 'w1', { id: 'm-ada', version: 1 }, 'r1')).rejects.toThrow('The invalid_role detail.')
})
