/**
 * Drives a real Orbit server through the Settings, Account, Admin and Docs calls the app's screens use, and puts back
 * everything it changes. It never changes the password, turns two-factor on or off, or revokes the session it uses.
 * Run: bun apps/mobile/scripts/api-check-settings.ts [server] [email] [password]
 * Defaults: http://127.0.0.1:18080, test@example.com, password.
 */
import * as account from '../src/data/account-api'
import * as admin from '../src/data/admin-api'
import { docBlocks, paragraphBlocks, visibleThreads } from '../src/data/docs'
import * as docs from '../src/data/docs-api'
import * as api from '../src/data/orbit'

const [server = 'http://127.0.0.1:18080', email = 'test@example.com', password = 'password'] = process.argv.slice(2)

function check(ok: boolean, message: string) {
  if (!ok) throw new Error(`FAILED: ${message}`)
  console.log(`ok   ${message}`)
}

async function signedInClient() {
  let token: string | null = null
  const client = api.createClient(server, () => token)
  const result = await api.signIn(client, email, password)
  if (result.kind !== 'signed_in') throw new Error('this check needs an account without two-factor sign-in')
  token = result.token
  return { client, user: result.user, token }
}

/** A 1×1 PNG. */
const PNG = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='), (char) => char.charCodeAt(0))

const { client, user, token } = await signedInClient()
check(true, `signed in as ${email}`)
const cleanup: (() => Promise<unknown>)[] = []

try {
  const [workspace] = await api.workspaces(client)
  if (!workspace) throw new Error('no workspace')
  const ws = workspace.id

  // ---- profile
  const before = await api.currentUser(client)
  const title = `Checker ${Date.now() % 10000}`
  const updated = await api.updateProfile(client, { display_name: before.display_name, title })
  check(updated.title === title, `profile update: title "${title}"`)
  const reverted = await api.updateProfile(client, { display_name: before.display_name, title: before.title ?? '' })
  check((reverted.title ?? null) === (before.title ?? null), 'profile update reverted')

  // ---- profile picture
  const withPicture = await account.uploadProfilePicture(client, new Blob([PNG], { type: 'image/png' }))
  check(Boolean(withPicture.avatar_url), `avatar upload: ${withPicture.avatar_url}`)
  const picture = await fetch(`${server}${withPicture.avatar_url}`, { headers: { Authorization: `Bearer ${token}` } })
  check(picture.status === 200 && (picture.headers.get('content-type') ?? '').startsWith('image/'), `the picture loads with the bearer token (${picture.headers.get('content-type')})`)
  if (!before.avatar_url) {
    const without = await account.removeProfilePicture(client)
    check(!without.avatar_url, 'avatar removed')
  } else {
    console.log('note the account had a picture before: it is replaced by the test image, remove it on the web if needed')
  }

  // ---- notifications
  const prefs = await account.notificationPrefs(client)
  const toggled = await account.saveNotificationPrefs(client, { ...prefs, mentions: !prefs.mentions })
  check(toggled.mentions === !prefs.mentions, `notification "mentions" toggled to ${toggled.mentions}`)
  const restored = await account.saveNotificationPrefs(client, prefs)
  check(JSON.stringify(restored) === JSON.stringify(prefs), 'notification choices restored')

  // ---- security and sessions
  const status = await account.twoFactor(client)
  check(typeof status.totp_enabled === 'boolean', `two-factor: ${account.twoFactorSummary(status)}`)
  const second = await signedInClient()
  const list = await account.sessions(client)
  const current = list.find((session) => session.current)
  check(Boolean(current) && list.length >= 2, `sessions: ${list.length}, the current one marked`)
  const others = await account.sessions(second.client)
  const secondId = others.find((session) => session.current)?.id
  check(Boolean(secondId) && secondId !== current?.id, 'a second sign-in is its own session')
  await account.revokeSession(client, secondId!)
  check(!(await account.sessions(client)).some((session) => session.id === secondId), 'the second session is revoked; this one still works')

  // ---- workspace
  const storage = await account.workspaceStorage(client, ws)
  check(storage.used_bytes >= 0, `workspace storage: ${account.storageUse(storage).label}`)
  const renamed = await account.renameWorkspace(client, workspace, `${workspace.name} (check)`)
  check(renamed.name === `${workspace.name} (check)`, 'workspace renamed')
  const back = await account.renameWorkspace(client, renamed, workspace.name)
  check(back.name === workspace.name, 'workspace name restored')
  const members = await api.members(client, ws)
  check(members.some((member) => member.user_id === user.id), `members: ${members.map((member) => `${member.display_name} (${account.roleLabel(member.role)})`).join(', ')}`)
  const me = members.find((member) => member.user_id === user.id)!
  check(account.memberActions(me, user.id).length === 0, 'no Manage menu on your own row')
  const options = await account.authOptions(client)
  check(typeof options.email_enabled === 'boolean', `auth options: email delivery ${options.email_enabled ? 'on' : 'off'}`)
  const invited = await account.invite(client, ws, `mobile-check-${Date.now()}@example.com`, 'member', 'manual')
  check(Boolean(invited.url), 'invitation created with a link')
  check((await account.pendingInvitations(client, ws)).some((item) => item.id === invited.invitation.id), 'the invitation is pending')
  await account.revokeInvitation(client, ws, invited.invitation.id)
  check(!(await account.pendingInvitations(client, ws)).some((item) => item.id === invited.invitation.id), 'invitation revoked')

  // ---- docs
  const page = await docs.newPage(client, ws, { private: true })
  cleanup.push(() => docs.deleteForever(client, ws, page).catch(() => undefined))
  check(page.private, 'private page created')
  const titled = await docs.savePage(client, ws, page, { title: 'Mobile check', icon: '🚀' })
  check(titled.title === 'Mobile check' && titled.icon === '🚀', 'page renamed with an icon')
  const written = await docs.savePage(client, ws, titled, { content: [...titled.content, ...paragraphBlocks('First paragraph\nSecond paragraph')] })
  const lines = docBlocks(written.content).map((block) => block.text)
  check(lines.includes('First paragraph') && lines.includes('Second paragraph'), `paragraphs written: ${JSON.stringify(lines)}`)
  check((await api.page(client, ws, page.id)).content.length === written.content.length, 'paragraphs read back')
  const coverUrl = await docs.uploadPageImage(client, ws, page.id, new Blob([PNG], { type: 'image/png' }))
  const covered = await docs.savePage(client, ws, written, { cover_url: coverUrl })
  check(covered.cover_url === coverUrl, `cover uploaded: ${coverUrl}`)
  const child = await docs.newPage(client, ws, { parent_id: page.id })
  check(child.parent_id === page.id && child.private, 'sub-page created under it')

  await docs.setFavorite(client, ws, page.id, true)
  check((await docs.favorites(client, ws)).includes(page.id), 'favorite added')
  await docs.setFavorite(client, ws, page.id, false)
  check(!(await docs.favorites(client, ws)).includes(page.id), 'favorite removed')

  const thread = await docs.commentOnPage(client, ws, page.id, paragraphBlocks('A comment from the app'))
  await docs.replyToThread(client, ws, page.id, thread.id, paragraphBlocks('A reply'))
  const threads = visibleThreads(await docs.threads(client, ws, page.id))
  check(threads[0]?.comments.map((comment) => comment.body_text.trim()).join(' | ') === 'A comment from the app | A reply', 'page comment and reply')

  const latest = await api.page(client, ws, page.id)
  await docs.trashPage(client, ws, latest)
  const trashed = (await docs.trash(client, ws)).find((item) => item.id === page.id)
  check(Boolean(trashed) && !(await api.pages(client, ws)).some((item) => item.id === child.id), 'page and sub-page moved to the trash')
  const restoredPage = await docs.restoreFromTrash(client, ws, trashed!)
  check(!restoredPage.deleted_at && (await api.pages(client, ws)).some((item) => item.id === child.id), 'restored with its sub-page')
  await docs.trashPage(client, ws, restoredPage)
  const again = (await docs.trash(client, ws)).find((item) => item.id === page.id)!
  check(again.can_purge, 'the trashed page can be deleted forever')
  await docs.deleteForever(client, ws, again)
  check(!(await docs.trash(client, ws)).some((item) => item.id === page.id), 'deleted forever')
  cleanup.pop()

  const teamspace = await docs.newTeamspace(client, ws, 'Mobile check space')
  cleanup.push(() => docs.removeTeamspace(client, ws, teamspace).catch(() => undefined))
  const renamedSpace = await docs.saveTeamspace(client, ws, teamspace, { name: 'Mobile check space 2', icon: '🧪' })
  check(renamedSpace.name === 'Mobile check space 2' && renamedSpace.icon === '🧪', 'teamspace created and renamed')
  await docs.removeTeamspace(client, ws, renamedSpace)
  check(!(await api.teamspaces(client, ws)).some((item) => item.id === teamspace.id), 'teamspace deleted')
  cleanup.pop()

  // ---- admin (reads only)
  if (user.installation_admin) {
    const users = await admin.adminUsers(client, '')
    check(users.items.some((item) => item.id === user.id), `admin users: ${users.items.length}${users.next ? '+' : ''}`)
    check(admin.adminActions(users.items.find((item) => item.id === user.id)!, user).length === 0, 'no Manage menu on your own account')
    const audit = await admin.auditEvents(client)
    check(audit.items.length > 0, `audit log: latest ${audit.items[0]?.action} by ${audit.items[0] ? admin.auditActor(audit.items[0]) : '-'}`)
    check(Array.isArray(await admin.backups(client)), 'backups listed')
    const kinds = await admin.jobLimits(client)
    check(kinds.length > 0, `job limits: ${kinds.map((kind) => `${admin.jobLabel(kind.kind)} ${admin.limitText(kind.concurrency)}`).join(', ')}`)
  } else {
    console.log('skip admin reads: the account is not an installation admin')
  }
} finally {
  for (const step of cleanup.reverse()) await step()
  await api.signOut(client)
  console.log('signed out')
}
