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

/* the chat reference webhooks: webhook-compatible embeds posted by webhooks */
export interface EmbedField {
  name: string
  value: string
  inline?: boolean
}

export interface Embed {
  title?: string
  description?: string
  url?: string
  /** 0xRRGGBB integer. */
  color?: number
  timestamp?: string
  footer?: { text: string; iconUrl?: string }
  image?: { url: string }
  thumbnail?: { url: string }
  author?: { name: string; url?: string; iconUrl?: string }
  fields?: EmbedField[]
}

/* the chat reference AttachmentInfo */
export interface Attachment {
  id: string
  fileName: string
  mimeType: string
  fileSize: number
  url: string
}

export interface Webhook {
  id: string
  name: string
  channelId: string
  token: string
  createdAt: string
  iconUrl: string | null
}

export interface ChatMessage {
  id: string
  channelId: string
  authorId: string
  authorType: 'user' | 'discord' | 'github' | 'system' | 'webhook'
  externalAuthor?: { name: string; source: string }
  /** authorType 'webhook': display name override + icon, plus optional embeds. */
  webhookName?: string
  webhookIconUrl?: string | null
  embeds?: Embed[]
  attachments?: Attachment[]
  /** the chat reference pins: who/when pinned; the timeline shows a "pinned a message" notice until hidden */
  pinnedAt?: string | null
  pinnedBy?: string | null
  pinNoticeHidden?: boolean
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
  /** The current user follows this thread (root messages only). */
  threadFollowed?: boolean
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
  webhooks: Webhook[]
  /** channelId → users currently typing (mock realtime; expires = epoch ms) */
  typingUsers: Record<string, Array<{ userId: string; expires: number }>>

  notifications: Notification[]
}
