import type { PresenceMap } from '@/features/realtime/presence'
import type {
  Attachment,
  Category,
  ChatEvent,
  Conversation,
  ConversationKind,
  ConversationState,
  FollowedThread,
  Message,
  MessagePage,
  NotifyLevel,
  SearchPage,
  ThreadPage,
  ThreadState,
} from './types'

export interface MessageCursor {
  /** Messages older than this id. */
  before?: string
  /** Messages newer than this id. */
  after?: string
  /** A window with this message in the middle (jump to message, first unread). */
  around?: string
  limit?: number
}

export interface SendMessageInput {
  conversationId: string
  threadRootId?: string | null
  body: string
  attachments?: Attachment[]
  alsoInChannel?: boolean
  /** Made by the caller; the `message.created` echo carries it. */
  nonce: string
}

export interface ChannelInput {
  name: string
  topic?: string
  categoryId?: string | null
  kind: Exclude<ConversationKind, 'dm'>
  memberIds?: string[]
}

export interface SearchInput {
  query: string
  conversationId?: string
  authorId?: string
  hasFile?: boolean
  cursor?: string
}

export interface UploadOptions {
  onProgress?: (fraction: number) => void
  signal?: AbortSignal
}

/**
 * Everything the chat UI needs from a backend, for one workspace and the signed-in user. The hooks in `queries.ts` and
 * `mutations.ts` are the only callers. All functions reject with a `ChatError`.
 */
export interface ChatClient {
  /** Conversations the user can see: joined ones, and every public channel (for Browse). Archived ones are left out. */
  listConversations(): Promise<Conversation[]>
  listCategories(): Promise<Category[]>
  listStates(): Promise<ConversationState[]>
  /** The main list: root messages, system rows and `alsoInChannel` replies. Without a cursor: the newest page. */
  listMessages(conversationId: string, cursor?: MessageCursor): Promise<MessagePage>
  getThread(rootId: string, cursor?: MessageCursor): Promise<ThreadPage>
  /** Roots that have replies, newest reply first. */
  listThreads(conversationId: string): Promise<Message[]>
  /** Unread first, then newest reply first. */
  listFollowedThreads(): Promise<FollowedThread[]>
  listPins(conversationId: string): Promise<Message[]>
  /** Messages that have attachments, newest first. */
  listFiles(conversationId: string): Promise<Message[]>
  searchMessages(input: SearchInput): Promise<SearchPage>
  /** Every member who does not show as offline, with their status. */
  getPresence(): Promise<PresenceMap>

  sendMessage(input: SendMessageInput): Promise<Message>
  editMessage(messageId: string, body: string): Promise<Message>
  deleteMessage(messageId: string): Promise<void>
  /** Adds (`on`) or takes back the user's reaction. Doing either twice changes nothing. */
  setReaction(messageId: string, emoji: string, on: boolean): Promise<Message>
  setPinned(messageId: string, pinned: boolean): Promise<Message>

  /** Reads the conversation up to its newest message. */
  markRead(conversationId: string): Promise<ConversationState>
  /** Moves the read cursor to just before this message. */
  markUnread(messageId: string): Promise<ConversationState>
  markThreadRead(rootId: string): Promise<ThreadState>
  /** Every conversation and followed thread. Returns the previous states, so the caller can offer Undo. */
  markAllRead(): Promise<{ states: ConversationState[]; threads: ThreadState[] }>
  restoreRead(previous: { states: ConversationState[]; threads: ThreadState[] }): Promise<void>
  setNotify(conversationId: string, notify: NotifyLevel): Promise<ConversationState>
  setFavorite(conversationId: string, favorite: boolean): Promise<ConversationState>
  setThreadFollow(rootId: string, following: boolean): Promise<ThreadState>

  createChannel(input: ChannelInput): Promise<Conversation>
  updateChannel(conversationId: string, patch: Partial<ChannelInput>): Promise<Conversation>
  archiveChannel(conversationId: string): Promise<void>
  joinChannel(conversationId: string): Promise<Conversation>
  leaveChannel(conversationId: string): Promise<void>
  addMembers(conversationId: string, userIds: string[]): Promise<Conversation>
  removeMember(conversationId: string, userId: string): Promise<Conversation>
  /** Returns the existing DM for this exact set of members (plus the current user), or makes one. */
  openDm(userIds: string[]): Promise<Conversation>

  createCategory(name: string): Promise<Category>
  renameCategory(categoryId: string, name: string): Promise<Category>
  /** Its channels move to no category. */
  deleteCategory(categoryId: string): Promise<void>
  /** One step up or down among its siblings. */
  /** Puts a category before the category `beforeId`, or at the end. */
  placeCategory(categoryId: string, beforeId: string | null): Promise<void>
  /** Puts a channel into a category (`null`: no category), before the channel `beforeId` or at the end. */
  placeChannel(conversationId: string, categoryId: string | null, beforeId: string | null): Promise<void>

  uploadAttachment(file: File, options?: UploadOptions): Promise<Attachment>
  sendTyping(conversationId: string, threadRootId?: string | null): void
  /** The tab went idle (no input for a while) or is in use again. Only a change is sent. */
  setIdle(idle: boolean): void
  /** The window has been out of focus for a while, or has the focus again. Only a change is sent. */
  setAway(away: boolean): void

  subscribe(listener: (event: ChatEvent) => void): () => void
}
