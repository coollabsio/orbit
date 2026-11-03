/**
 * Drives a real Orbit server through the same data functions the app's screens use, then cleans up what it created.
 * Run: bun apps/mobile/scripts/api-check.ts [server] [email] [password]
 * Defaults: http://127.0.0.1:18080, test@example.com, password.
 */
import { deleteTask } from '@orbit/api/generated/sdk.gen'

import { buildMessageRows, chatSections, chatUnreadCount, type MessagePages, newNonce, upsertMessage } from '../src/data/chat'
import { chatActions, LiveCursor, parseLiveFrame, parseWorkspaceEvent, socketUrl, workspaceEventKeys } from '../src/data/live'
import * as api from '../src/data/orbit'
import { reconnectingSocket, type SocketLike } from '../src/data/socket'
import { groupTasksByStatus, identifier, isVersionConflict, myOpenTasks, projectStatuses } from '../src/data/tasks'

const [server = 'http://127.0.0.1:18080', email = 'test@example.com', password = 'password'] = process.argv.slice(2)

function check(ok: boolean, message: string) {
  if (!ok) throw new Error(`FAILED: ${message}`)
  console.log(`ok   ${message}`)
}

const waitFor = async (predicate: () => boolean, ms = 5000) => {
  const end = Date.now() + ms
  while (!predicate() && Date.now() < end) await new Promise((resolve) => setTimeout(resolve, 50))
  return predicate()
}

let token: string | null = null
const client = api.createClient(server, () => token)
const signIn = await api.signIn(client, email, password)
check(signIn.kind === 'signed_in', `signed in as ${email} without an Origin header`)
if (signIn.kind !== 'signed_in') process.exit(1)
token = signIn.token
const userId = signIn.user.id
const cleanup: (() => Promise<unknown>)[] = []

/** Bun's WebSocket takes the upgrade headers in its options, as React Native's does in its third argument. */
const bearerSocket = (extra: Record<string, string> = {}) => (url: string) =>
  new WebSocket(url, { headers: { Authorization: `Bearer ${token}`, ...extra } } as unknown as string[]) as unknown as SocketLike

try {
  check((await api.currentUser(client)).id === userId, 'GET /auth/me with the bearer token')
  const workspaces = await api.workspaces(client)
  const workspace = workspaces[0]!
  check(workspaces.length > 0, `workspaces: ${workspaces.map((item) => item.name).join(', ')}`)
  const ws = workspace.id

  // live sockets first, so they see the writes below
  const workspaceEvents: string[] = []
  let after: string | undefined
  const events = reconnectingSocket({
    url: () => socketUrl(server, `/api/v1/workspaces/${ws}/events`, { after }),
    create: bearerSocket(),
    onMessage: (raw) => {
      const event = parseWorkspaceEvent(raw)
      if (!event) return
      after = event.sequence
      workspaceEvents.push(`${event.kind}#${event.sequence} -> ${workspaceEventKeys(event, ws).map((key) => key[0]).join(',')}`)
    },
  })
  const cursor = new LiveCursor()
  const frames: string[] = []
  const chatEvents: ReturnType<typeof chatActions> = []
  const live = reconnectingSocket({
    url: () => socketUrl(server, `/api/v1/workspaces/${ws}/live`, cursor.query()),
    create: bearerSocket(),
    onMessage: (raw) => {
      const frame = parseLiveFrame(raw)
      if (!frame) return
      cursor.read(frame)
      frames.push(frame.type)
      if (frame.type === 'event' && frame.topic === 'chat') chatEvents.push(...chatActions(frame.event as never, ws))
    },
  })
  cleanup.push(async () => {
    events.close()
    live.close()
  })
  check(await waitFor(() => frames.includes('hello') && frames.includes('resync')), `live socket with bearer: ${frames.join(', ')}`)

  // React Native's native WebSocket adds `Origin: <scheme>://<host>` of the URL by itself: does the server take it?
  const origin = new URL(server).origin
  const probe = new WebSocket(socketUrl(server, `/api/v1/workspaces/${ws}/live`), { headers: { Authorization: `Bearer ${token}`, Origin: origin } } as unknown as string[])
  const probeResult = await new Promise<string>((resolve) => {
    probe.onmessage = (event) => resolve(`first frame ${parseLiveFrame(event.data)?.type ?? 'other'}`)
    probe.onclose = (event) => resolve(`closed ${event.code}`)
    probe.onerror = () => resolve('handshake refused')
    setTimeout(() => resolve('no answer in 3s'), 3000)
  })
  probe.close()
  console.log(`info live socket with bearer and Origin ${origin}: ${probeResult}`)

  // tasks
  const [tasks, projects] = await Promise.all([api.tasks(client, ws), api.projects(client, ws)])
  const statuses = await api.statuses(client, ws, projects.map((project) => project.id))
  const groups = groupTasksByStatus(tasks, statuses)
  check(statuses.length > 0, `${tasks.length} tasks, ${projects.length} projects, ${statuses.length} statuses`)
  console.log(`info groups: ${groups.map((group) => `${group.name} (${group.tasks.length})`).join(', ')}`)
  console.log(`info my open tasks: ${myOpenTasks(tasks, statuses, userId, 5).map((item) => identifier(item, projects)).join(', ') || 'none'}`)

  const created = await api.newTask(client, ws, {
    title: 'Mobile API check (safe to delete)',
    description: '',
    projectId: projects[0]!.id,
    statusId: projectStatuses(statuses, projects[0]!.id)[0]!.id,
    priority: 'none',
    assigneeIds: [],
  })
  check(created.title === 'Mobile API check (safe to delete)', `created task ${identifier(created, projects)}`)
  const target = projectStatuses(statuses, created.project_id).find((status) => status.id !== created.status_id)!
  const moved = await api.changeTask(client, ws, created.id, { status_id: target.id, expected_version: created.version })
  check(moved.status_id === target.id && moved.version > created.version, `status -> ${target.name} with expected_version ${created.version}`)
  const stale = await api.changeTask(client, ws, created.id, { status_id: created.status_id, expected_version: created.version }).then(
    () => null,
    (error: unknown) => error,
  )
  check(isVersionConflict(stale), 'a stale expected_version is a version conflict')
  const comment = await api.addComment(client, ws, created.id, 'Comment from the **mobile** API check')
  check((await api.comments(client, ws, created.id)).some((item) => item.id === comment.id), 'added and listed a task comment')
  check(await waitFor(() => workspaceEvents.length > 0), `workspace events: ${workspaceEvents.slice(0, 3).join(' | ')}`)
  await api.removeComment(client, ws, comment)
  check((await api.comments(client, ws, created.id)).length === 0, 'deleted the comment')

  // chat
  const [conversations, categories, states] = await Promise.all([api.conversations(client, ws), api.chatCategories(client, ws), api.chatStates(client, ws)])
  const sections = chatSections(conversations, categories, states, userId)
  check(sections.length > 0, `chat sections: ${sections.map((section) => `${section.title} (${section.conversations.length})`).join(', ')}`)
  console.log(`info unread chat count: ${chatUnreadCount(conversations, states, userId)}`)
  const general = conversations.find((item) => item.name === 'general')!
  const page = await api.messages(client, ws, general.id)
  check(page.after === null, `#general newest page: ${page.items.length} messages, ${buildMessageRows(page.items).length} rows`)
  const nonce = newNonce()
  const sent = await api.sendMessage(client, ws, general.id, '[mobile API check] test message, deleted right after', nonce)
  check(sent.nonce === nonce, 'sent a message to #general')
  check(
    await waitFor(() => chatEvents.some((action) => action.kind === 'upsert-message' && action.message.id === sent.id)),
    'the live socket delivered message.created for it',
  )
  const cached: MessagePages = { pages: [page], pageParams: [undefined] }
  check(upsertMessage(cached, sent).pages[0]!.items.at(-1)!.id === sent.id, 'the echo patches the cached newest page')
  const read = await api.markConversationRead(client, ws, general.id)
  check(read.unread_count === 0, 'marked #general read')
  await api.deleteMessage(client, ws, sent.id)
  check(!(await api.messages(client, ws, general.id)).items.some((item) => item.id === sent.id), 'deleted the test message')

  // inbox
  const notifications = await api.notifications(client, ws)
  const unread = (await api.unreadNotifications(client, ws)).length
  check(unread === notifications.filter((item) => !item.read_at).length || notifications.length === 100, `notifications: ${notifications.length}, unread ${unread}`)

  cleanup.push(async () => {
    await deleteTask({ client, path: { workspace_id: ws, task_id: created.id }, query: { expected_version: moved.version }, throwOnError: true })
    console.log(`ok   deleted test task ${identifier(created, projects)} (to the trash)`)
  })
} finally {
  for (const step of cleanup.reverse()) await step().catch((error: unknown) => console.log(`warn cleanup: ${error instanceof Error ? error.message : String(error)}`))
  await api.signOut(client)
  token = null
  console.log('ok   signed out')
}
