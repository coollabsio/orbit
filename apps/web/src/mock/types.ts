export interface User {
  id: string
  name: string
  handle: string
  email: string
  role: 'Owner' | 'Admin' | 'Member' | 'Visitor'
  color: string
  online: boolean
  title: string
}

export type TaskStatus = 'todo' | 'in_progress' | 'done' | 'cancelled'
export type TaskPriority = 'none' | 'low' | 'medium' | 'high' | 'urgent'

export interface Project {
  id: string
  name: string
  key: string
  color: string
}

export interface TaskComment {
  id: string
  authorId: string
  body: string
  createdAt: string
}

export interface TaskActivity {
  id: string
  actorId: string
  text: string
  createdAt: string
}

export interface Task {
  id: string
  identifier: string
  title: string
  description: string
  status: TaskStatus
  priority: TaskPriority
  assigneeId: string | null
  creatorId: string
  projectId: string
  labels: string[]
  dueAt: string | null
  createdAt: string
  updatedAt: string
  comments: TaskComment[]
  activity: TaskActivity[]
}

export interface Doc {
  id: string
  title: string
  icon: string | null
  parentId: string | null
  content: DocBlock[]
  createdBy: string
  updatedBy: string
  createdAt: string
  updatedAt: string
}

export interface DocBlock {
  id: string
  type: 'h1' | 'h2' | 'h3' | 'p' | 'bullet' | 'numbered' | 'quote' | 'code' | 'divider' | 'todo' | 'page'
  text: string
  checked?: boolean
  /** For 'page' blocks: the linked document id (reference-style page block). */
  refId?: string
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

export interface ChatCategory {
  id: string
  name: string
  /** Optional leading emoji (the chat reference: categories.emoji). Order = array order. */
  emoji?: string
}

export interface Channel {
  id: string
  name: string
  description: string
  /** Optional emoji shown instead of "#" (the chat reference: channels.emoji). Order = array order. */
  emoji?: string
  categoryId: string
  unreadCount: number
}

export interface ChatMessage {
  id: string
  channelId: string
  authorId: string
  authorType: 'user' | 'discord' | 'github' | 'system'
  externalAuthor?: { name: string; source: string }
  replyToId: string | null
  content: string
  reactions: Array<{ emoji: string; userIds: string[] }>
  createdAt: string
  editedAt: string | null
  pinned: boolean
  /** the chat reference threads: a thread is a message; replies point at the root via threadRootId. */
  threadRootId: string | null
  startsThread: boolean
  threadTitle: string | null
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
  projects: Project[]
  tasks: Task[]
  docs: Doc[]
  mailFolders: MailFolder[]
  mailThreads: MailThread[]
  chatCategories: ChatCategory[]
  channels: Channel[]
  chatMessages: ChatMessage[]
  /** channelId → users currently typing (mock realtime; expires = epoch ms) */
  typingUsers: Record<string, Array<{ userId: string; expires: number }>>

  notifications: Notification[]
}
