import { getState, nextId, updateState } from './store'
import type {
  ChatMessage,
  Doc,
  DocBlock,
  MailThread,
  Task,
  TaskPriority,
  TaskStatus,
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

export function sendChatMessage(channelId: string, content: string, replyToId: string | null = null) {
  updateState((s) => {
    const message: ChatMessage = {
      id: nextId('cm'),
      channelId,
      authorId: s.currentUserId,
      authorType: 'user',
      replyToId,
      content,
      reactions: [],
      createdAt: now(),
    }
    return { ...s, chatMessages: [...s.chatMessages, message] }
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
