import { getState, nextId, updateState } from './store'
import type { Attachment } from '@/lib/attachmentLib'
import type { MailThread, MailFolder, User } from './types'

const now = () => new Date().toISOString()

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

export function sendReply(threadId: string, body: string, attachments: Attachment[] = []) {
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
                  attachments,
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

/* ---------- user profile ---------- */

export function updateUserProfile(userId: string, patch: Partial<Pick<User, 'name' | 'email' | 'title'>>) {
  updateState((s) => ({
    ...s,
    users: s.users.map((u) => (u.id === userId ? { ...u, ...patch } : u)),
  }))
}

/* ---------- mail folders ---------- */

export function createMailFolder(name: string): MailFolder {
  const folder: MailFolder = { id: nextId('f'), name: name.trim(), icon: 'folder', custom: true }
  updateState((s) => ({ ...s, mailFolders: [...s.mailFolders, folder] }))
  return folder
}
