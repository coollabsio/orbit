export interface User {
  id: string
  name: string
  handle: string
  email: string
  role: 'Owner' | 'Admin' | 'Member' | 'Visitor'
  color: string
  online: boolean
  title: string
  /** Custom role ids (server roles); order follows the roles list. */
  roleIds: string[]
}

export interface Role {
  id: string
  name: string
  color: string
  position: number
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

export interface DirectMessage {
  id: string
  participantId: string
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

/** Uploaded emoji, used anywhere as :name:. */
export interface CustomEmoji {
  id: string
  name: string
  url: string
  createdBy: string
  createdAt: string
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
  roles: Role[]
  mailFolders: MailFolder[]
  mailThreads: MailThread[]
  chatCategories: ChatCategory[]
  channels: Channel[]
  directMessages: DirectMessage[]
  chatMessages: ChatMessage[]
  webhooks: Webhook[]
  customEmojis: CustomEmoji[]
  /** channelId → users currently typing (mock realtime; expires = epoch ms) */
  typingUsers: Record<string, Array<{ userId: string; expires: number }>>

  notifications: Notification[]
}
