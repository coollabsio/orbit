/** Ids are opaque strings; message ids sort by creation time. Times are integer milliseconds. */

export type ConversationKind = 'public' | 'private' | 'dm'
export type NotifyLevel = 'all' | 'mentions' | 'muted'

export interface Conversation {
  id: string
  kind: ConversationKind
  /** Empty for a DM: its title is made from the other members' names. */
  name: string
  topic: string
  categoryId: string | null
  /** Order inside its category. */
  position: number
  memberIds: string[]
  /** The current user is a member. A non-member can still read a public channel. */
  isMember: boolean
  /** `#general`: everybody is in it and it cannot be left, archived or made private. */
  isDefault: boolean
  archived: boolean
  createdBy: string
  createdAt: number
  lastMessageAt: number | null
}

export interface Category {
  id: string
  name: string
  position: number
}

/** Same field names as the shared `Attachments` and `ImageViewer` components use. */
export interface Attachment {
  id: string
  fileName: string
  mimeType: string
  fileSize: number
  url: string
  /** Known for images, so the list can reserve the space before they load. */
  width?: number
  height?: number
}

export interface Reaction {
  emoji: string
  userIds: string[]
}

export interface Mentions {
  userIds: string[]
  channel: boolean
  here: boolean
}

/** The newest reply of a thread, shown under its root in the conversation. */
export interface ReplyPreview {
  authorId: string
  body: string
  createdAt: number
}

/** `pin`, `join` and `leave` are one-line system rows; `authorId` is the member who did it. */
export type MessageKind = 'message' | 'pin' | 'join' | 'leave'

export interface Message {
  id: string
  conversationId: string
  /** Set on a thread reply. A root message has `null`. */
  threadRootId: string | null
  kind: MessageKind
  authorId: string
  /** Markdown source. Mentions are tokens: `<@userId>`, `<#conversationId>`, `<!channel>`, `<!here>`. */
  body: string
  mentions: Mentions
  createdAt: number
  editedAt: number | null
  /** A deleted root that has replies stays in the list with an empty body. */
  deleted: boolean
  attachments: Attachment[]
  reactions: Reaction[]
  pinned: boolean
  /** A thread reply that also shows in the conversation's main list. */
  alsoInChannel: boolean
  /** Echo of the sender's nonce, so the optimistic row can be replaced. */
  nonce: string | null
  /** Thread summary; meaningful on a root message only. */
  replyCount: number
  lastReplyAt: number | null
  replyUserIds: string[]
  lastReply: ReplyPreview | null
  /** Client only: set on a message that the server has not confirmed. */
  sendState?: 'sending' | 'failed'
}

/** The current user's state in one conversation. Counts come from the client implementation, never from the UI. */
export interface ConversationState {
  conversationId: string
  lastReadMessageId: string | null
  /** Unread messages in the main list (thread replies count on `ThreadState`). */
  unreadCount: number
  mentionCount: number
  notify: NotifyLevel
  favorite: boolean
}

/** The current user's state in one thread. Exists once the user follows it or has opened it. */
export interface ThreadState {
  rootId: string
  conversationId: string
  following: boolean
  lastReadReplyId: string | null
  unreadReplies: number
  mentionCount: number
}

export interface FollowedThread {
  root: Message
  conversationId: string
  state: ThreadState
  lastReply: Message | null
}

/** A page of messages in ascending time order. A `null` cursor means there is nothing more in that direction. */
export interface MessagePage {
  items: Message[]
  before: string | null
  after: string | null
}

export interface ThreadPage extends MessagePage {
  root: Message
  state: ThreadState | null
}

export interface SearchHit {
  message: Message
  conversationId: string
  /** `[start, end)` offsets in `message.body` to highlight. */
  ranges: [number, number][]
}

export interface SearchPage {
  items: SearchHit[]
  cursor: string | null
}

export type ChatErrorCode = 'not_found' | 'forbidden' | 'too_long' | 'conflict' | 'upload_failed' | 'offline'

export class ChatError extends Error {
  readonly code: ChatErrorCode
  /** The request did not reach the server, or a proxy answered for it: the same request can go again. */
  readonly unreached: boolean

  constructor(code: ChatErrorCode, message: string, unreached = false) {
    super(message)
    this.name = 'ChatError'
    this.code = code
    this.unreached = unreached
  }
}

export type ConnectionStatus = 'connected' | 'reconnecting'

export type ChatEvent =
  | { type: 'message.created'; message: Message }
  /** Edits, reactions, pins and thread summary changes. */
  | { type: 'message.updated'; message: Message }
  | { type: 'message.deleted'; conversationId: string; messageId: string; threadRootId: string | null }
  /** Also sent with `archived: true`, so an open archived channel can show its notice; it leaves the sidebar. */
  | { type: 'conversation.changed'; conversation: Conversation }
  /** The current user lost access (left or was removed from a private channel). */
  | { type: 'conversation.removed'; conversationId: string }
  | { type: 'categories.changed'; categories: Category[] }
  | { type: 'state.changed'; state: ConversationState }
  | { type: 'thread.changed'; state: ThreadState }
  | { type: 'typing'; conversationId: string; threadRootId: string | null; userId: string }
  | { type: 'presence'; userId: string; online: boolean }
  | { type: 'connection'; status: ConnectionStatus }
  /** The client missed events: refetch every chat query. */
  | { type: 'resync' }

export const MESSAGE_MAX_LENGTH = 4000
