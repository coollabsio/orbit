/**
 * Every call the app makes to the Orbit API, as plain functions over the generated SDK. No React and no React Native, so
 * screens, the live hook and a Bun script share them.
 */
import { createOrbitClient } from '@orbit/api/client'
import {
  createComment,
  createProject as createProjectRequest,
  createTask,
  deleteChatMessage,
  deleteComment,
  deleteTask,
  getPage,
  getTask,
  listChatCategories,
  listChatConversations,
  listChatMessages,
  listChatStates,
  listComments,
  listLabels,
  listMembers,
  listNotifications,
  listPages,
  listProjects,
  listStatuses,
  listTaskActivity,
  listTasks,
  listTeamspaces,
  listWorkspaces,
  login,
  loginSecondFactor,
  logout,
  me,
  readAllNotifications,
  readChatConversation,
  readNotification,
  sendChatMessage,
  updateMe,
  updateTask,
} from '@orbit/api/generated/sdk.gen'
import type {
  AuthUserResponse,
  CategoryRecord,
  CommentRecord,
  ConversationRecord,
  ConversationStateRecord,
  LabelRecord,
  LoginResponse,
  MemberRecord,
  MessagePage,
  MessageRecord,
  NotificationRecord,
  Page,
  PageAuditEvent,
  PageSummary,
  ProjectBody,
  ProjectRecord,
  StatusRecord,
  TaskRecord,
  TaskUpdateBody,
  Teamspace,
  UpdateMeBody,
  WorkspaceRecord,
} from '@orbit/api/generated/types.gen'

export type OrbitClient = ReturnType<typeof createOrbitClient>
export type User = AuthUserResponse
export type {
  CategoryRecord,
  CommentRecord,
  ConversationRecord,
  ConversationStateRecord,
  LabelRecord,
  MemberRecord,
  MessagePage,
  MessageRecord,
  NotificationRecord,
  Page,
  PageSummary,
  ProjectRecord,
  StatusRecord,
  TaskRecord,
  Teamspace,
  UpdateMeBody,
  WorkspaceRecord,
}

/** One entry of a task's audit trail (`GET …/tasks/{id}/activity`). */
export type TaskActivityEvent = PageAuditEvent['items'][number]

/** `orbit.example.com` → `https://orbit.example.com`. Keeps an explicit `http://` for local servers. */
export function normalizeServer(input: string): string {
  const trimmed = input.trim().replace(/\/+$/, '')
  return /^https?:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`
}

/**
 * React Native sends no `Origin` header, so the server treats the app as a native client: sign-in returns the session
 * token in the body, and every later request sends it as a bearer token.
 */
export function createClient(server: string, token: () => string | null, onUnauthorized?: () => void, onPaymentRequired?: () => void): OrbitClient {
  return createOrbitClient({ baseUrl: server, token, onUnauthorized, onPaymentRequired })
}

export type SignIn = { kind: 'signed_in'; token: string; user: User } | { kind: 'two_factor'; twoFactorToken: string }

/** A sign-in answer (password, second factor or provider exchange) as the app's `SignIn`. */
export function signedIn(body: LoginResponse | { two_factor_token: string }): SignIn {
  if ('two_factor_token' in body) return { kind: 'two_factor', twoFactorToken: body.two_factor_token }
  if (!body.session_token) throw new Error('The server did not return a session token. Update the Orbit server.')
  return { kind: 'signed_in', token: body.session_token, user: body.user }
}

export async function signIn(client: OrbitClient, email: string, password: string): Promise<SignIn> {
  const { data } = await login({ client, body: { email, password }, throwOnError: true })
  return signedIn(data)
}

export async function signInSecondFactor(client: OrbitClient, twoFactorToken: string, code: string): Promise<SignIn> {
  const { data } = await loginSecondFactor({ client, body: { two_factor_token: twoFactorToken, code }, throwOnError: true })
  return signedIn(data)
}

export async function currentUser(client: OrbitClient): Promise<User> {
  return (await me({ client, throwOnError: true })).data
}

export async function updateProfile(client: OrbitClient, body: UpdateMeBody): Promise<User> {
  return (await updateMe({ client, body, throwOnError: true })).data
}

export async function signOut(client: OrbitClient): Promise<void> {
  await logout({ client, throwOnError: true })
}

export async function workspaces(client: OrbitClient): Promise<WorkspaceRecord[]> {
  return (await listWorkspaces({ client, throwOnError: true })).data
}

/** Follows `next_cursor` until the server has no more. */
async function allPages<T>(page: (cursor: string | undefined) => Promise<{ items: T[]; next_cursor?: string | null }>): Promise<T[]> {
  const items: T[] = []
  let cursor: string | undefined
  do {
    const next = await page(cursor)
    items.push(...next.items)
    cursor = next.next_cursor ?? undefined
  } while (cursor)
  return items
}

const at = (client: OrbitClient, workspaceId: string) => ({ client, path: { workspace_id: workspaceId }, throwOnError: true as const })

// ---- tasks

/** Every task, or the tasks of a server view (`mine`, `my_week`, `overdue` …) when `view` is set. */
export function tasks(client: OrbitClient, workspaceId: string, view?: string): Promise<TaskRecord[]> {
  return allPages(async (cursor) => (await listTasks({ ...at(client, workspaceId), query: { cursor, limit: 100, view } })).data)
}

export interface TaskQuery {
  view?: string
  assignee_id?: string
  sort?: string
  order?: string
  limit?: number
}

/** The first page of a task query: the Home cards show it and whether there are more. */
export async function taskPage(client: OrbitClient, workspaceId: string, query: TaskQuery): Promise<{ items: TaskRecord[]; more: boolean }> {
  const { data } = await listTasks({ ...at(client, workspaceId), query })
  return { items: data.items, more: Boolean(data.next_cursor) }
}

export async function taskActivity(client: OrbitClient, workspaceId: string, taskId: string): Promise<TaskActivityEvent[]> {
  return (await listTaskActivity({ client, path: { workspace_id: workspaceId, task_id: taskId }, throwOnError: true })).data.items
}

export async function task(client: OrbitClient, workspaceId: string, taskId: string): Promise<TaskRecord> {
  return (await getTask({ client, path: { workspace_id: workspaceId, task_id: taskId }, throwOnError: true })).data
}

export async function changeTask(client: OrbitClient, workspaceId: string, taskId: string, body: TaskUpdateBody): Promise<TaskRecord> {
  return (await updateTask({ client, path: { workspace_id: workspaceId, task_id: taskId }, body, throwOnError: true })).data
}

export interface NewTask {
  title: string
  description: string
  projectId: string
  statusId: string
  priority: string
  assigneeIds: string[]
}

export async function newTask(client: OrbitClient, workspaceId: string, input: NewTask): Promise<TaskRecord> {
  const body = {
    title: input.title,
    description: input.description,
    project_id: input.projectId,
    status_id: input.statusId,
    priority: input.priority,
    assignee_ids: input.assigneeIds,
  }
  return (await createTask({ ...at(client, workspaceId), body })).data
}

/** Moves the task to the trash, as the web's Delete. */
export async function trashTask(client: OrbitClient, workspaceId: string, task: Pick<TaskRecord, 'id' | 'version'>): Promise<void> {
  await deleteTask({ client, path: { workspace_id: workspaceId, task_id: task.id }, query: { expected_version: task.version }, throwOnError: true })
}

export function labels(client: OrbitClient, workspaceId: string): Promise<LabelRecord[]> {
  return allPages(async (cursor) => (await listLabels({ ...at(client, workspaceId), query: { cursor, limit: 100 } })).data)
}

export function projects(client: OrbitClient, workspaceId: string): Promise<ProjectRecord[]> {
  return allPages(async (cursor) => (await listProjects({ ...at(client, workspaceId), query: { cursor, limit: 100 } })).data)
}

/** A new project from its name, as the web's `projectDraft`: a key of up to five letters and digits, and violet. */
export function projectDraft(name: string): ProjectBody {
  return { name, key: name.replace(/[^a-z0-9]/gi, '').slice(0, 5).toUpperCase() || 'PROJ', color: '#8b5cf6' }
}

export async function createProject(client: OrbitClient, workspaceId: string, body: ProjectBody): Promise<ProjectRecord> {
  return (await createProjectRequest({ ...at(client, workspaceId), body })).data
}

/** The statuses of every project, in one list. */
export async function statuses(client: OrbitClient, workspaceId: string, projectIds: readonly string[]): Promise<StatusRecord[]> {
  const perProject = await Promise.all(
    projectIds.map((projectId) =>
      allPages(async (cursor) => (await listStatuses({ client, path: { workspace_id: workspaceId, project_id: projectId }, query: { cursor, limit: 100 }, throwOnError: true })).data),
    ),
  )
  return perProject.flat()
}

export function comments(client: OrbitClient, workspaceId: string, taskId: string): Promise<CommentRecord[]> {
  return allPages(async (cursor) => (await listComments({ client, path: { workspace_id: workspaceId, task_id: taskId }, query: { cursor, limit: 100 }, throwOnError: true })).data)
}

export async function addComment(client: OrbitClient, workspaceId: string, taskId: string, body: string): Promise<CommentRecord> {
  return (await createComment({ client, path: { workspace_id: workspaceId, task_id: taskId }, body: { body }, throwOnError: true })).data
}

export async function removeComment(client: OrbitClient, workspaceId: string, comment: CommentRecord): Promise<void> {
  await deleteComment({
    client,
    path: { workspace_id: workspaceId, task_id: comment.task_id, comment_id: comment.id },
    query: { expected_version: comment.version },
    throwOnError: true,
  })
}

export function members(client: OrbitClient, workspaceId: string): Promise<MemberRecord[]> {
  return allPages(async (cursor) => (await listMembers({ ...at(client, workspaceId), query: { cursor, limit: 100 } })).data)
}

// ---- chat

export async function conversations(client: OrbitClient, workspaceId: string): Promise<ConversationRecord[]> {
  return (await listChatConversations(at(client, workspaceId))).data
}

export async function chatCategories(client: OrbitClient, workspaceId: string): Promise<CategoryRecord[]> {
  return (await listChatCategories(at(client, workspaceId))).data
}

export async function chatStates(client: OrbitClient, workspaceId: string): Promise<ConversationStateRecord[]> {
  return (await listChatStates(at(client, workspaceId))).data
}

/** Without `before`: the newest page. Items are oldest first. */
export async function messages(client: OrbitClient, workspaceId: string, conversationId: string, before?: string): Promise<MessagePage> {
  return (
    await listChatMessages({ client, path: { workspace_id: workspaceId, conversation_id: conversationId }, query: { before, limit: 50 }, throwOnError: true })
  ).data
}

export async function sendMessage(client: OrbitClient, workspaceId: string, conversationId: string, body: string, nonce: string): Promise<MessageRecord> {
  return (await sendChatMessage({ client, path: { workspace_id: workspaceId, conversation_id: conversationId }, body: { body, nonce }, throwOnError: true })).data.result
}

export async function deleteMessage(client: OrbitClient, workspaceId: string, messageId: string): Promise<void> {
  await deleteChatMessage({ client, path: { workspace_id: workspaceId, message_id: messageId }, throwOnError: true })
}

export async function markConversationRead(client: OrbitClient, workspaceId: string, conversationId: string): Promise<ConversationStateRecord> {
  return (await readChatConversation({ client, path: { workspace_id: workspaceId, conversation_id: conversationId }, throwOnError: true })).data.result
}

// ---- inbox

export async function notifications(client: OrbitClient, workspaceId: string): Promise<NotificationRecord[]> {
  return (await listNotifications({ ...at(client, workspaceId), query: { limit: 100 } })).data.items
}

export function unreadNotifications(client: OrbitClient, workspaceId: string): Promise<NotificationRecord[]> {
  return allPages(async (cursor) => (await listNotifications({ ...at(client, workspaceId), query: { unread: true, cursor, limit: 100 } })).data)
}

export async function markNotificationRead(client: OrbitClient, workspaceId: string, notificationId: string): Promise<NotificationRecord> {
  return (await readNotification({ client, path: { workspace_id: workspaceId, notification_id: notificationId }, throwOnError: true })).data
}

export async function markAllNotificationsRead(client: OrbitClient, workspaceId: string): Promise<void> {
  await readAllNotifications(at(client, workspaceId))
}

// ---- docs

export async function pages(client: OrbitClient, workspaceId: string): Promise<PageSummary[]> {
  return (await listPages(at(client, workspaceId))).data.items
}

export async function page(client: OrbitClient, workspaceId: string, pageId: string): Promise<Page> {
  return (await getPage({ client, path: { workspace_id: workspaceId, page_id: pageId }, throwOnError: true })).data
}

export async function teamspaces(client: OrbitClient, workspaceId: string): Promise<Teamspace[]> {
  return (await listTeamspaces(at(client, workspaceId))).data.items
}
