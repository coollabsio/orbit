import { getState, nextId, updateState } from './store'
import type {
  Attachment,
  Channel,
  ChatCategory,
  ChatMessage,
  CustomEmoji,
  Doc,
  DocBlock,
  MailThread,
  Task,
  TaskPriority,
  StatusCategory,
  TaskStatusDef,
  MailFolder,
  Project,
  Role,
  Webhook,
  User,
} from './types'

import { defaultStatusOf, projectStatuses } from '../components/workspace/taskMeta'

const now = () => new Date().toISOString()

/* ---------- tasks ---------- */

function touchTask(taskId: string, patch: Partial<Task>, activityText?: string, activityStatusId?: string) {
  updateState((s) => ({
    ...s,
    tasks: s.tasks.map((t) =>
      t.id === taskId
        ? {
            ...t,
            ...patch,
            updatedAt: now(),
            activity: activityText
              ? [
                  ...t.activity,
                  {
                    id: nextId('ta'),
                    actorId: s.currentUserId,
                    text: activityText,
                    createdAt: now(),
                    ...(activityStatusId ? { statusId: activityStatusId } : {}),
                  },
                ]
              : t.activity,
          }
        : t,
    ),
  }))
}

export function setTaskStatus(taskId: string, statusId: string) {
  const name = getState().statuses.find((s) => s.id === statusId)?.name ?? 'Unknown'
  touchTask(taskId, { statusId }, `changed status to ${name}`, statusId)
}

/** Drag and drop on the board: new column (status) and manual position inside it. */
export function moveTask(taskId: string, statusId: string, position: number) {
  const s = getState()
  const task = s.tasks.find((t) => t.id === taskId)
  if (!task) return
  if (task.statusId !== statusId) {
    const name = s.statuses.find((st) => st.id === statusId)?.name ?? 'Unknown'
    touchTask(taskId, { statusId, position }, `changed status to ${name}`, statusId)
  } else {
    updateState((prev) => ({ ...prev, tasks: prev.tasks.map((t) => (t.id === taskId ? { ...t, position } : t)) }))
  }
}

export function setTaskPriority(taskId: string, priority: TaskPriority) {
  touchTask(taskId, { priority }, `set priority to ${priority}`)
}

/** Adds or removes one assignee (tasks can have several). */
export function toggleTaskAssignee(taskId: string, userId: string) {
  const s = getState()
  const task = s.tasks.find((t) => t.id === taskId)
  if (!task) return
  const name = s.users.find((u) => u.id === userId)?.name ?? 'someone'
  const assigned = task.assigneeIds.includes(userId)
  touchTask(
    taskId,
    { assigneeIds: assigned ? task.assigneeIds.filter((id) => id !== userId) : [...task.assigneeIds, userId] },
    assigned ? `unassigned ${name}` : `assigned ${name}`,
  )
}

export function setTaskDueAt(taskId: string, dueAt: string | null) {
  const label = dueAt ? new Date(dueAt).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) : null
  touchTask(taskId, { dueAt }, label ? `set the due date to ${label}` : 'removed the due date')
}

export function setTaskTitle(taskId: string, title: string) {
  touchTask(taskId, { title })
}

export function setTaskDescription(taskId: string, description: string) {
  touchTask(taskId, { description })
}

export function addTaskAttachments(taskId: string, attachments: Attachment[]) {
  const task = getState().tasks.find((t) => t.id === taskId)
  if (!task || attachments.length === 0) return
  touchTask(
    taskId,
    { attachments: [...task.attachments, ...attachments] },
    attachments.length === 1 ? `attached ${attachments[0].fileName}` : `attached ${attachments.length} files`,
  )
}

export function removeTaskAttachment(taskId: string, attachmentId: string) {
  const task = getState().tasks.find((t) => t.id === taskId)
  if (!task) return
  touchTask(taskId, { attachments: task.attachments.filter((a) => a.id !== attachmentId) })
}

export function addTaskComment(taskId: string, body: string, parentId?: string, attachments: Attachment[] = []) {
  updateState((s) => ({
    ...s,
    tasks: s.tasks.map((t) =>
      t.id === taskId
        ? {
            ...t,
            updatedAt: now(),
            comments: [
              ...t.comments,
              {
                id: nextId('tc'),
                authorId: s.currentUserId,
                body,
                createdAt: now(),
                ...(parentId ? { parentId } : {}),
                ...(attachments.length > 0 ? { attachments } : {}),
              },
            ],
          }
        : t,
    ),
  }))
}

export function editTaskComment(taskId: string, commentId: string, body: string) {
  updateState((s) => ({
    ...s,
    tasks: s.tasks.map((t) =>
      t.id === taskId
        ? { ...t, comments: t.comments.map((c) => (c.id === commentId ? { ...c, body, editedAt: now() } : c)) }
        : t,
    ),
  }))
}

/** Deletes a comment; deleting a top-level comment also removes its replies. */
export function deleteTaskComment(taskId: string, commentId: string) {
  updateState((s) => ({
    ...s,
    tasks: s.tasks.map((t) =>
      t.id === taskId ? { ...t, comments: t.comments.filter((c) => c.id !== commentId && c.parentId !== commentId) } : t,
    ),
  }))
}

/** Adds the label when missing, removes it when present. */
export function toggleTaskLabel(taskId: string, label: string) {
  const task = getState().tasks.find((t) => t.id === taskId)
  if (!task) return
  const has = task.labels.includes(label)
  touchTask(
    taskId,
    { labels: has ? task.labels.filter((l) => l !== label) : [...task.labels, label] },
    has ? `removed label ${label}` : `added label ${label}`,
  )
}

export function createTask(input: {
  title: string
  projectId: string
  statusId?: string
  priority?: TaskPriority
  assigneeIds?: string[]
}): Task {
  const s = getState()
  const project = s.projects.find((p) => p.id === input.projectId) ?? s.projects[0]
  const count = s.tasks.filter((t) => t.projectId === project.id).length
  const task: Task = {
    id: nextId('t'),
    identifier: `${project.key}-${100 + count + 1}`,
    title: input.title,
    description: '',
    statusId: input.statusId ?? defaultStatusOf(s.statuses, project.id)?.id ?? '',
    position: Math.min(0, ...s.tasks.map((t) => t.position)) - 1,
    priority: input.priority ?? 'none',
    assigneeIds: input.assigneeIds ?? [],
    creatorId: s.currentUserId,
    projectId: project.id,
    labels: [],
    attachments: [],
    dueAt: null,
    createdAt: now(),
    updatedAt: now(),
    comments: [],
    activity: [{ id: nextId('ta'), actorId: s.currentUserId, text: 'created this task', createdAt: now() }],
  }
  updateState((prev) => ({ ...prev, tasks: [task, ...prev.tasks] }))
  return task
}

/* ---------- projects ---------- */

export function updateProject(projectId: string, patch: Partial<Pick<Project, 'name' | 'key' | 'color'>>) {
  updateState((s) => ({ ...s, projects: s.projects.map((p) => (p.id === projectId ? { ...p, ...patch } : p)) }))
}

/** Removes the project and every task in it. */
export function deleteProject(projectId: string) {
  updateState((s) => ({
    ...s,
    projects: s.projects.filter((p) => p.id !== projectId),
    statuses: s.statuses.filter((st) => st.projectId !== projectId),
    tasks: s.tasks.filter((t) => t.projectId !== projectId),
  }))
}

/* ---------- statuses ---------- */

export function createStatus(
  projectId: string,
  input: { name: string; category: StatusCategory; color: string; description?: string },
): TaskStatusDef {
  const siblings = getState().statuses.filter((s) => s.projectId === projectId && s.category === input.category)
  const status: TaskStatusDef = {
    id: nextId('st'),
    projectId,
    name: input.name,
    description: input.description ?? '',
    color: input.color,
    category: input.category,
    position: siblings.length > 0 ? Math.max(...siblings.map((s) => s.position)) + 1 : 0,
  }
  updateState((s) => ({ ...s, statuses: [...s.statuses, status] }))
  return status
}

export function updateStatus(statusId: string, patch: Partial<Pick<TaskStatusDef, 'name' | 'description' | 'color'>>) {
  updateState((s) => ({ ...s, statuses: s.statuses.map((st) => (st.id === statusId ? { ...st, ...patch } : st)) }))
}

/** Removes a status; its tasks move to the next status of the same category, else the project default. */
export function deleteStatus(statusId: string) {
  const s = getState()
  const status = s.statuses.find((st) => st.id === statusId)
  if (!status) return
  const remaining = s.statuses.filter((st) => st.id !== statusId)
  const fallback =
    projectStatuses(remaining, status.projectId).find((st) => st.category === status.category) ??
    defaultStatusOf(remaining, status.projectId)
  if (!fallback) return
  updateState((prev) => ({
    ...prev,
    statuses: remaining,
    tasks: prev.tasks.map((t) => (t.statusId === statusId ? { ...t, statusId: fallback.id, updatedAt: now() } : t)),
  }))
}

/** Moves a status before/after another one of the same project and category. */
export function reorderStatus(statusId: string, targetId: string, position: 'before' | 'after') {
  const s = getState()
  const moving = s.statuses.find((st) => st.id === statusId)
  const target = s.statuses.find((st) => st.id === targetId)
  if (!moving || !target || moving.id === target.id) return
  if (moving.projectId !== target.projectId || moving.category !== target.category) return
  const ordered = projectStatuses(s.statuses, moving.projectId)
    .filter((st) => st.category === moving.category && st.id !== moving.id)
  const index = ordered.findIndex((st) => st.id === target.id) + (position === 'after' ? 1 : 0)
  ordered.splice(index, 0, moving)
  const positions = new Map(ordered.map((st, i) => [st.id, i]))
  updateState((prev) => ({
    ...prev,
    statuses: prev.statuses.map((st) => (positions.has(st.id) ? { ...st, position: positions.get(st.id)! } : st)),
  }))
}

/* ---------- custom emojis ---------- */

export function createCustomEmoji(name: string, url: string): CustomEmoji {
  const s = getState()
  // shortcodes are unique: "party", "party_2", …
  let unique = name
  let n = 2
  while (s.customEmojis.some((e) => e.name === unique)) unique = `${name}_${n++}`
  const emoji: CustomEmoji = { id: nextId('ce'), name: unique, url, createdBy: s.currentUserId, createdAt: now() }
  updateState((prev) => ({ ...prev, customEmojis: [...prev.customEmojis, emoji] }))
  return emoji
}

export function renameCustomEmoji(emojiId: string, name: string) {
  updateState((s) => ({ ...s, customEmojis: s.customEmojis.map((e) => (e.id === emojiId ? { ...e, name } : e)) }))
}

export function deleteCustomEmoji(emojiId: string) {
  updateState((s) => ({ ...s, customEmojis: s.customEmojis.filter((e) => e.id !== emojiId) }))
}

/* ---------- sessions ---------- */

export function revokeSession(sessionId: string) {
  updateState((s) => ({ ...s, sessions: s.sessions.filter((session) => session.id !== sessionId || session.current) }))
}

export function revokeOtherSessions() {
  updateState((s) => ({ ...s, sessions: s.sessions.filter((session) => session.current) }))
}

/* ---------- docs ---------- */

export function updateDocTitle(docId: string, title: string) {
  updateState((s) => ({
    ...s,
    docs: s.docs.map((d) =>
      d.id === docId ? { ...d, title, updatedAt: now(), updatedBy: s.currentUserId } : d,
    ),
  }))
}

export function updateDocIcon(docId: string, icon: string | null) {
  updateState((s) => ({
    ...s,
    docs: s.docs.map((d) => (d.id === docId ? { ...d, icon, updatedAt: now(), updatedBy: s.currentUserId } : d)),
  }))
}

export function updateDocCover(docId: string, patch: { cover?: string | null; coverPos?: string | null }) {
  updateState((s) => ({
    ...s,
    docs: s.docs.map((d) => (d.id === docId ? { ...d, ...patch, updatedAt: now(), updatedBy: s.currentUserId } : d)),
  }))
}

export function updateDocContent(docId: string, content: DocBlock[]) {
  updateState((s) => ({
    ...s,
    docs: s.docs.map((d) =>
      d.id === docId ? { ...d, content, updatedAt: now(), updatedBy: s.currentUserId } : d,
    ),
  }))
}

export function createDoc(parentId: string | null): Doc {
  const s = getState()
  const doc: Doc = {
    id: nextId('d'),
    title: 'Untitled',
    icon: null,
    parentId,
    content: [{ id: nextId('db'), type: 'p', text: '' }],
    createdBy: s.currentUserId,
    updatedBy: s.currentUserId,
    createdAt: now(),
    updatedAt: now(),
  }
  updateState((prev) => ({ ...prev, docs: [...prev.docs, doc] }))
  return doc
}

/** Deletes a page and every page below it. */
export function deleteDoc(docId: string) {
  updateState((s) => {
    const gone = new Set([docId])
    let grew = true
    while (grew) {
      grew = false
      for (const d of s.docs) {
        if (d.parentId && gone.has(d.parentId) && !gone.has(d.id)) {
          gone.add(d.id)
          grew = true
        }
      }
    }
    return { ...s, docs: s.docs.filter((d) => !gone.has(d.id)) }
  })
}

/** Moves a page under `parentId` (null = root), inserted before `beforeId` (null = last sibling). */
export function moveDoc(docId: string, parentId: string | null, beforeId: string | null = null) {
  updateState((s) => {
    const doc = s.docs.find((d) => d.id === docId)
    if (!doc || docId === parentId) return s
    // never move a page into its own subtree
    let cursor = parentId
    while (cursor) {
      if (cursor === docId) return s
      cursor = s.docs.find((d) => d.id === cursor)?.parentId ?? null
    }
    const moved = { ...doc, parentId, updatedAt: now(), updatedBy: s.currentUserId }
    const rest = s.docs.filter((d) => d.id !== docId)
    const at = beforeId ? rest.findIndex((d) => d.id === beforeId) : -1
    const docs = at === -1 ? [...rest, moved] : [...rest.slice(0, at), moved, ...rest.slice(at)]
    return { ...s, docs }
  })
}

/* ---------- mail ---------- */

function patchThread(threadId: string, patch: Partial<MailThread>) {
  updateState((s) => ({
    ...s,
    mailThreads: s.mailThreads.map((t) => (t.id === threadId ? { ...t, ...patch } : t)),
  }))
}

export function setThreadRead(threadId: string, read: boolean) {
  patchThread(threadId, { unread: !read })
}

export function toggleThreadStar(threadId: string) {
  const thread = getState().mailThreads.find((t) => t.id === threadId)
  if (thread) patchThread(threadId, { starred: !thread.starred })
}

export function moveThread(threadId: string, folderId: string) {
  patchThread(threadId, { folderId })
}

export function sendReply(threadId: string, body: string) {
  updateState((s) => {
    const me = s.users.find((u) => u.id === s.currentUserId)
    return {
      ...s,
      mailThreads: s.mailThreads.map((t) =>
        t.id === threadId
          ? {
              ...t,
              snippet: body.slice(0, 90),
              updatedAt: now(),
              messages: [
                ...t.messages,
                {
                  id: nextId('mm'),
                  from: { name: me?.name ?? 'Me', email: me?.email ?? 'me@team.dev' },
                  to: [t.messages[t.messages.length - 1]?.from.email ?? ''],
                  body,
                  createdAt: now(),
                },
              ],
            }
          : t,
      ),
    }
  })
}

export function composeMail(input: { to: string; subject: string; body: string }) {
  updateState((s) => {
    const me = s.users.find((u) => u.id === s.currentUserId)
    const thread: MailThread = {
      id: nextId('m'),
      folderId: 'f_sent',
      subject: input.subject || '(no subject)',
      snippet: input.body.slice(0, 90),
      unread: false,
      starred: false,
      hasAttachment: false,
      updatedAt: now(),
      messages: [
        {
          id: nextId('mm'),
          from: { name: me?.name ?? 'Me', email: me?.email ?? 'me@team.dev' },
          to: [input.to],
          body: input.body,
          createdAt: now(),
        },
      ],
    }
    return { ...s, mailThreads: [thread, ...s.mailThreads] }
  })
}

/* ---------- chat ---------- */


/** Mock realtime: after the current user sends a message, another member "types" for 3s. */
export interface SendChatMessageOptions {
  threadRootId?: string | null
  startsThread?: boolean
  threadTitle?: string | null
  attachments?: Attachment[]
}

export function sendChatMessage(
  channelId: string,
  content: string,
  replyToId: string | null = null,
  options: SendChatMessageOptions = {},
): string {
  const id = nextId('cm')
  updateState((s) => {
    const message: ChatMessage = {
      id,
      channelId,
      authorId: s.currentUserId,
      authorType: 'user',
      replyToId,
      content,
      reactions: [],
      createdAt: now(),
      editedAt: null,
      pinned: false,
      threadRootId: options.threadRootId ?? null,
      startsThread: options.startsThread ?? false,
      threadTitle: options.threadTitle ?? null,
      attachments: options.attachments?.length ? options.attachments : undefined,
      threadFollowed: options.startsThread ? true : undefined,
    }
    // auto-follow: replying into a thread follows its root
    const chatMessages = options.threadRootId
      ? s.chatMessages.map((m) => (m.id === options.threadRootId ? { ...m, threadFollowed: true } : m))
      : s.chatMessages
    return { ...s, chatMessages: [...chatMessages, message] }
  })
  simulateTypingReply(channelId)
  return id
}

export function followThread(rootId: string, follow: boolean) {
  updateState((s) => ({
    ...s,
    chatMessages: s.chatMessages.map((m) => (m.id === rootId ? { ...m, threadFollowed: follow } : m)),
  }))
}

/** the chat reference NewThreadPanel: a thread-starter root (title as content) plus the first reply. */
export function createThread(channelId: string, title: string, firstMessage: string): string {
  const rootId = sendChatMessage(channelId, title.trim(), null, { startsThread: true, threadTitle: title.trim() })
  sendChatMessage(channelId, firstMessage, null, { threadRootId: rootId })
  return rootId
}

export function renameThread(rootId: string, title: string) {
  updateState((s) => ({
    ...s,
    chatMessages: s.chatMessages.map((m) => (m.id === rootId ? { ...m, threadTitle: title.trim() || null } : m)),
  }))
}

function simulateTypingReply(channelId: string) {
  const s = getState()
  const others = s.users.filter((u) => u.id !== s.currentUserId && u.online)
  const member = others[(channelId.length + s.chatMessages.length) % Math.max(others.length, 1)]
  if (!member) return
  const expires = Date.now() + 3000
  updateState((prev) => ({
    ...prev,
    typingUsers: { ...prev.typingUsers, [channelId]: [{ userId: member.id, expires }] },
  }))
  window.setTimeout(() => {
    updateState((prev) => ({
      ...prev,
      typingUsers: {
        ...prev.typingUsers,
        [channelId]: (prev.typingUsers[channelId] ?? []).filter((t) => t.expires > Date.now()),
      },
    }))
  }, 3200)
}

export function editChatMessage(messageId: string, content: string) {
  updateState((s) => ({
    ...s,
    chatMessages: s.chatMessages.map((m) =>
      m.id === messageId ? { ...m, content, editedAt: now() } : m,
    ),
  }))
}

export function deleteChatMessage(messageId: string) {
  updateState((s) => ({
    ...s,
    chatMessages: s.chatMessages
      .filter((m) => m.id !== messageId && m.threadRootId !== messageId)
      .map((m) => (m.replyToId === messageId ? { ...m, replyToId: null } : m)),
  }))
}



export function togglePinMessage(messageId: string) {
  updateState((s) => ({
    ...s,
    chatMessages: s.chatMessages.map((m) =>
      m.id === messageId
        ? m.pinned
          ? { ...m, pinned: false, pinnedAt: null, pinnedBy: null, pinNoticeHidden: false }
          : { ...m, pinned: true, pinnedAt: now(), pinnedBy: s.currentUserId, pinNoticeHidden: false }
        : m,
    ),
  }))
}

/** the chat reference hide_pin_notice: remove the "pinned a message" timeline row, keep the pin. */
export function hidePinNotice(messageId: string) {
  updateState((s) => ({
    ...s,
    chatMessages: s.chatMessages.map((m) => (m.id === messageId ? { ...m, pinNoticeHidden: true } : m)),
  }))
}

/** the chat reference api.attachments.delete */
export function deleteAttachment(messageId: string, attachmentId: string) {
  updateState((s) => ({
    ...s,
    chatMessages: s.chatMessages.map((m) =>
      m.id === messageId ? { ...m, attachments: (m.attachments ?? []).filter((a) => a.id !== attachmentId) } : m,
    ),
  }))
}

export function createChannel(categoryId: string, name: string, description = '') {
  const id = nextId('c')
  updateState((s) => ({
    ...s,
    channels: [...s.channels, { id, name, description, categoryId, unreadCount: 0 }],
  }))
  return id
}

export function updateChannel(channelId: string, patch: { name?: string; description?: string; emoji?: string }) {
  updateState((s) => ({
    ...s,
    channels: s.channels.map((c) => (c.id === channelId ? { ...c, ...patch } : c)),
  }))
}

export function deleteChannel(channelId: string) {
  updateState((s) => ({
    ...s,
    channels: s.channels.filter((c) => c.id !== channelId),
    chatMessages: s.chatMessages.filter((m) => m.channelId !== channelId),
  }))
}

export function createChatCategory(name: string, emoji = '') {
  updateState((s) => ({
    ...s,
    chatCategories: [...s.chatCategories, { id: nextId('cc'), name, emoji }],
  }))
}

export function updateChatCategory(categoryId: string, patch: { name?: string; emoji?: string }) {
  updateState((s) => ({
    ...s,
    chatCategories: s.chatCategories.map((c) => (c.id === categoryId ? { ...c, ...patch } : c)),
  }))
}

/** the chat reference reorder contract: the full ordered id list replaces the current order. */
export function reorderChatCategories(orderedIds: string[]) {
  updateState((s) => {
    const byId = new Map(s.chatCategories.map((c) => [c.id, c]))
    const ordered = orderedIds.map((id) => byId.get(id)).filter((c): c is ChatCategory => !!c)
    const rest = s.chatCategories.filter((c) => !orderedIds.includes(c.id))
    return { ...s, chatCategories: [...ordered, ...rest] }
  })
}

/** Reorder channels inside one category; channels of other categories keep their order. */
export function reorderChannels(categoryId: string, orderedIds: string[]) {
  updateState((s) => {
    const byId = new Map(s.channels.map((c) => [c.id, c]))
    const ordered = orderedIds.map((id) => byId.get(id)).filter((c): c is Channel => !!c && c.categoryId === categoryId)
    const next: Channel[] = []
    let inserted = false
    for (const channel of s.channels) {
      if (channel.categoryId !== categoryId) {
        next.push(channel)
      } else if (!inserted) {
        next.push(...ordered)
        inserted = true
      }
    }
    return { ...s, channels: next }
  })
}

export function deleteChatCategory(categoryId: string) {
  updateState((s) => {
    const channelIds = s.channels.filter((c) => c.categoryId === categoryId).map((c) => c.id)
    return {
      ...s,
      chatCategories: s.chatCategories.filter((c) => c.id !== categoryId),
      channels: s.channels.filter((c) => c.categoryId !== categoryId),
      chatMessages: s.chatMessages.filter((m) => !channelIds.includes(m.channelId)),
    }
  })
}

export function toggleReaction(messageId: string, emoji: string) {
  updateState((s) => ({
    ...s,
    chatMessages: s.chatMessages.map((msg) => {
      if (msg.id !== messageId) return msg
      const existing = msg.reactions.find((r) => r.emoji === emoji)
      const me = s.currentUserId
      let reactions
      if (!existing) {
        reactions = [...msg.reactions, { emoji, userIds: [me] }]
      } else if (existing.userIds.includes(me)) {
        reactions = msg.reactions
          .map((r) => (r.emoji === emoji ? { ...r, userIds: r.userIds.filter((u) => u !== me) } : r))
          .filter((r) => r.userIds.length > 0)
      } else {
        reactions = msg.reactions.map((r) =>
          r.emoji === emoji ? { ...r, userIds: [...r.userIds, me] } : r,
        )
      }
      return { ...msg, reactions }
    }),
  }))
}

export function markChannelRead(channelId: string) {
  updateState((s) => ({
    ...s,
    channels: s.channels.map((c) => (c.id === channelId ? { ...c, unreadCount: 0 } : c)),
  }))
}

/* ---------- notifications ---------- */

export function markNotificationRead(notificationId: string) {
  updateState((s) => ({
    ...s,
    notifications: s.notifications.map((n) =>
      n.id === notificationId ? { ...n, readAt: n.readAt ?? now() } : n,
    ),
  }))
}

export function markAllNotificationsRead() {
  updateState((s) => ({
    ...s,
    notifications: s.notifications.map((n) => ({ ...n, readAt: n.readAt ?? now() })),
  }))
}

/* ---------- team members ---------- */

export function setUserRole(userId: string, role: User['role']) {
  updateState((s) => ({
    ...s,
    users: s.users.map((u) => (u.id === userId ? { ...u, role } : u)),
  }))
}

export function removeUser(userId: string) {
  updateState((s) => {
    if (userId === s.currentUserId) return s
    return { ...s, users: s.users.filter((u) => u.id !== userId) }
  })
}

export function updateUserProfile(userId: string, patch: Partial<Pick<User, 'name' | 'email' | 'title'>>) {
  updateState((s) => ({
    ...s,
    users: s.users.map((u) => (u.id === userId ? { ...u, ...patch } : u)),
  }))
}

/* ---------- webhooks (the chat reference api.webhooks) ---------- */

function randomToken() {
  return Array.from({ length: 20 }, () => Math.floor(Math.random() * 16).toString(16)).join('')
}

export function createWebhook(channelId: string): Webhook {
  const webhook: Webhook = {
    id: nextId('wh'),
    name: 'New Webhook',
    channelId,
    token: randomToken(),
    createdAt: now(),
    iconUrl: null,
  }
  updateState((s) => ({ ...s, webhooks: [webhook, ...s.webhooks] }))
  return webhook
}

export function updateWebhook(id: string, patch: { name?: string; channelId?: string; iconUrl?: string | null }) {
  updateState((s) => ({
    ...s,
    webhooks: s.webhooks.map((w) => (w.id === id ? { ...w, ...patch } : w)),
  }))
}

export function deleteWebhook(id: string) {
  updateState((s) => ({ ...s, webhooks: s.webhooks.filter((w) => w.id !== id) }))
}

/* ---------- mail folders ---------- */

export function createMailFolder(name: string): MailFolder {
  const folder: MailFolder = { id: nextId('f'), name: name.trim(), icon: 'folder', custom: true }
  updateState((s) => ({ ...s, mailFolders: [...s.mailFolders, folder] }))
  return folder
}

/* ---------- roles ---------- */

export function createRole(name: string, color: string): Role {
  const role: Role = { id: nextId('r'), name, color, position: getState().roles.length }
  updateState((s) => ({ ...s, roles: [...s.roles, role] }))
  return role
}

export function updateRole(roleId: string, patch: { name?: string; color?: string }) {
  updateState((s) => ({ ...s, roles: s.roles.map((r) => (r.id === roleId ? { ...r, ...patch } : r)) }))
}

export function deleteRole(roleId: string) {
  updateState((s) => ({
    ...s,
    roles: s.roles.filter((r) => r.id !== roleId).map((r, position) => ({ ...r, position })),
    users: s.users.map((u) => ({ ...u, roleIds: u.roleIds.filter((id) => id !== roleId) })),
  }))
}

export function reorderRoles(orderedIds: string[]) {
  updateState((s) => {
    const byId = new Map(s.roles.map((r) => [r.id, r]))
    const ordered = orderedIds.map((id) => byId.get(id)).filter((r): r is Role => !!r)
    return { ...s, roles: ordered.map((r, position) => ({ ...r, position })) }
  })
}

export function assignMemberRoles(userId: string, roleIds: string[]) {
  updateState((s) => ({ ...s, users: s.users.map((u) => (u.id === userId ? { ...u, roleIds } : u)) }))
}

/* ---------- workspace ---------- */

export function updateWorkspace(patch: { name?: string; iconUrl?: string | null }) {
  updateState((s) => ({ ...s, workspace: { ...s.workspace, ...patch } }))
}

