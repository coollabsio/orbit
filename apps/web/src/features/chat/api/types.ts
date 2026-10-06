import type { PresenceEntry } from '@/features/realtime/presence'

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
  /** A DM the user opened with themselves alone: their notes. A DM whose other members left is not one. */
  selfDm: boolean
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

/** What an external URL shows as a card. Image URLs are always `https`. */
export interface LinkPreview {
  url: string
  /** `x`: a post on X. `youtube`: `videoId` plays it. `image`: the URL is an image. */
  kind: 'link' | 'x' | 'youtube' | 'image'
  siteName?: string
  title?: string
  description?: string
  imageUrl?: string
  /** The page asks for a wide image. */
  largeImage: boolean
  iconUrl?: string
  authorName?: string
  authorHandle?: string
  authorAvatarUrl?: string
  videoId?: string
  /** The video of a post on X (an `mp4`); `imageUrl` is its poster. */
  videoUrl?: string
  videoWidth?: number
  videoHeight?: number
  /** Milliseconds. */
  createdAt?: number
  replies?: number
  reposts?: number
  likes?: number
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
  /** The reply has a sticker. Missing on a row cached before stickers existed. */
  sticker?: boolean
  createdAt: number
}

/** `pin`, `join` and `leave` are one-line system rows; `authorId` is the member who did it. */
export type MessageKind = 'message' | 'pin' | 'join' | 'leave'

/** The message that a reply quotes, shown on one line above the reply. */
export interface ReplyQuote {
  id: string
  authorId: string
  /** The start of the quoted message's stored body, with its mention tokens. */
  body: string
  /** The quoted message has a sticker. Missing on a row cached before stickers existed. */
  sticker?: boolean
}

/** Where a forwarded message was copied from. The original may be gone, or in a conversation the reader cannot open. */
export interface ForwardOrigin {
  messageId: string
  conversationId: string
  authorId: string
  /** When the original message was sent. */
  createdAt: number
}

/** The sticker of a message: a custom image of the workspace, shown large. `url` is the image, served by Orbit. */
export interface MessageSticker {
  id: string
  name: string
  url: string
}

export interface Message {
  id: string
  conversationId: string
  /** Set on a thread reply. A root message has `null`. */
  threadRootId: string | null
  /** Set on an inline reply: the message it quotes. It stays when that message is deleted. */
  replyToId: string | null
  /** The quote of `replyToId`; `null` when that message was deleted. */
  replyTo: ReplyQuote | null
  /**
   * Set on a forward: a new message of the forwarder whose body and attachments are a copy of the original. It cannot
   * be edited and does not change with the original. A row cached before forwards existed has no such field.
   */
  forwarded: ForwardOrigin | null
  /** Set on a message that was sent with a sticker; its body may be empty. It stays when the sticker is deleted. */
  stickerId: string | null
  /** The sticker of `stickerId`; `null` when it was deleted. A row cached before stickers existed has neither field. */
  sticker: MessageSticker | null
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

/** `ended`: the session ended or the user lost access, so nothing reconnects. */
export type ConnectionStatus = 'connected' | 'reconnecting' | 'ended'

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
  /** What the others see of a member changed; `null` is offline. */
  | { type: 'presence'; userId: string; presence: PresenceEntry | null }
  /** The signed-in user changed their own status, in this tab or another: fetch the current user again. */
  | { type: 'self.changed' }
  /** A custom emoji of the workspace was added or deleted: fetch the list again. */
  | { type: 'emoji.changed' }
  /** A sticker of the workspace was added or deleted: fetch the list again. */
  | { type: 'stickers.changed' }
  | { type: 'connection'; status: ConnectionStatus }
  /** The client missed events: refetch every chat query. */
  | { type: 'resync' }

export const MESSAGE_MAX_LENGTH = 4000
