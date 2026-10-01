import type { Attachment } from '@/lib/attachmentLib'

export interface User {
  id: string
  name: string
  handle: string
  email: string
  role: 'Owner' | 'Admin' | 'Member' | 'Visitor'
  color: string
  online: boolean
  title: string
  roleIds: string[]
}

export interface MailFolder {
  id: string
  name: string
  icon: 'inbox' | 'send' | 'note' | 'trash' | 'archive' | 'star' | 'folder'
  /** user-created folder (listed under "Custom folders") */
  custom?: boolean
}

export interface MailMessage {
  id: string
  from: { name: string; email: string }
  to: string[]
  body: string
  createdAt: string
  attachments?: Attachment[]
}

export interface MailThread {
  id: string
  folderId: string
  subject: string
  snippet: string
  messages: MailMessage[]
  unread: boolean
  starred: boolean
  hasAttachment: boolean
  updatedAt: string
}

/** A row of the Home dashboard's "Active channels" card. */
export interface Channel {
  id: string
  name: string
  description: string
  unreadCount: number
}

export interface Notification {
  id: string
  type: 'mention' | 'assignment' | 'comment' | 'github' | 'system'
  actorId: string | null
  title: string
  body: string
  resourceType: 'task' | 'doc' | 'channel' | 'mail' | null
  resourceId: string | null
  readAt: string | null
  createdAt: string
}

export interface AppState {
  currentUserId: string
  users: User[]
  mailFolders: MailFolder[]
  mailThreads: MailThread[]
  channels: Channel[]
  notifications: Notification[]
}
