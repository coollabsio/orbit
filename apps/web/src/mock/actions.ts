import { getState, nextId, updateState } from './store'
import type {
  Attachment,
  Channel,
  ChatCategory,
  ChatMessage,
  Doc,
  DocBlock,
  MailThread,
  Task,
  TaskPriority,
  TaskStatus,
  MailFolder,
  Webhook,
  User,
} from './types'

const now = () => new Date().toISOString()

/* ---------- tasks ---------- */

function touchTask(taskId: string, patch: Partial<Task>, activityText?: string) {
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
                  { id: nextId('ta'), actorId: s.currentUserId, text: activityText, createdAt: now() },
                ]
              : t.activity,
          }
        : t,
    ),
  }))
}

const statusLabel: Record<TaskStatus, string> = {
  todo: 'Todo',
  in_progress: 'In Progress',
  done: 'Done',
  cancelled: 'Cancelled',
}

export function setTaskStatus(taskId: string, status: TaskStatus) {
  touchTask(taskId, { status }, `changed status to ${statusLabel[status]}`)
}

export function setTaskPriority(taskId: string, priority: TaskPriority) {
  touchTask(taskId, { priority }, `set priority to ${priority}`)
}

export function setTaskAssignee(taskId: string, assigneeId: string | null) {
  const name = assigneeId ? getState().users.find((u) => u.id === assigneeId)?.name : null
  touchTask(taskId, { assigneeId }, name ? `assigned ${name}` : 'removed the assignee')
}

export function setTaskTitle(taskId: string, title: string) {
  touchTask(taskId, { title })
}

export function setTaskDescription(taskId: string, description: string) {
  touchTask(taskId, { description })
}

export function addTaskComment(taskId: string, body: string) {
  updateState((s) => ({
    ...s,
    tasks: s.tasks.map((t) =>
      t.id === taskId
        ? {
            ...t,
            updatedAt: now(),
            comments: [
              ...t.comments,
              { id: nextId('tc'), authorId: s.currentUserId, body, createdAt: now() },
            ],
          }
        : t,
    ),
  }))
}

export function createTask(input: { title: string; projectId: string; priority?: TaskPriority; assigneeId?: string | null }): Task {
  const s = getState()
  const project = s.projects.find((p) => p.id === input.projectId) ?? s.projects[0]
  const count = s.tasks.filter((t) => t.projectId === project.id).length
  const task: Task = {
    id: nextId('t'),
    identifier: `${project.key}-${100 + count + 1}`,
    title: input.title,
    description: '',
    status: 'todo',
    priority: input.priority ?? 'none',
    assigneeId: input.assigneeId ?? null,
    creatorId: s.currentUserId,
    projectId: project.id,
    labels: [],
    dueAt: null,
    createdAt: now(),
    updatedAt: now(),
    comments: [],
    activity: [{ id: nextId('ta'), actorId: s.currentUserId, text: 'created this task', createdAt: now() }],
  }
  updateState((prev) => ({ ...prev, tasks: [task, ...prev.tasks] }))
  return task
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

export function deleteDoc(docId: string) {
  updateState((s) => ({
    ...s,
    docs: s.docs.filter((d) => d.id !== docId && d.parentId !== docId),
  }))
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
