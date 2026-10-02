import type {
  ChatEvent as WireEvent,
  ChatFileRecord,
  ConversationRecord,
  ConversationStateRecord,
  FollowedThreadRecord,
  MessagePage as WireMessagePage,
  MessageRecord,
  ThreadPage as WireThreadPage,
  ThreadStateRecord,
} from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import {
  type Attachment,
  ChatError,
  type ChatErrorCode,
  type ChatEvent,
  type Conversation,
  type ConversationState,
  type FollowedThread,
  type Message,
  type MessagePage,
  type ThreadPage,
  type ThreadState,
} from './types'

/** The server sends RFC 3339 times; chat works in integer milliseconds. */
const millis = (time: string) => Date.parse(time)

/** `isMember` is the one field that depends on who looks, so the server leaves it to the client. */
export function toConversation(record: ConversationRecord, currentUserId: string): Conversation {
  return {
    id: record.id,
    kind: record.kind,
    name: record.name,
    topic: record.topic,
    categoryId: record.category_id,
    position: record.position,
    memberIds: record.member_ids,
    isMember: record.member_ids.includes(currentUserId),
    isDefault: record.is_default,
    archived: record.archived,
    createdBy: record.created_by,
    createdAt: millis(record.created_at),
    lastMessageAt: record.last_message_at === null ? null : millis(record.last_message_at),
  }
}

export function toAttachment(record: ChatFileRecord): Attachment {
  return {
    id: record.id,
    fileName: record.file_name,
    mimeType: record.mime_type,
    fileSize: record.size_bytes,
    url: record.url,
    ...(record.width !== null && record.height !== null ? { width: record.width, height: record.height } : {}),
  }
}

export function toMessage(record: MessageRecord): Message {
  return {
    id: record.id,
    conversationId: record.conversation_id,
    threadRootId: record.thread_root_id,
    kind: record.kind,
    authorId: record.author_id,
    body: record.body,
    mentions: { userIds: record.mentions.user_ids, channel: record.mentions.channel, here: record.mentions.here },
    createdAt: millis(record.created_at),
    editedAt: record.edited_at === null ? null : millis(record.edited_at),
    deleted: record.deleted,
    attachments: record.attachments.map(toAttachment),
    reactions: record.reactions.map((reaction) => ({ emoji: reaction.emoji, userIds: reaction.user_ids })),
    pinned: record.pinned,
    alsoInChannel: record.also_in_channel,
    nonce: record.nonce,
    replyCount: record.reply_count,
    lastReplyAt: record.last_reply_at === null ? null : millis(record.last_reply_at),
    replyUserIds: record.reply_user_ids,
    lastReply: record.last_reply && {
      authorId: record.last_reply.author_id,
      body: record.last_reply.body,
      createdAt: millis(record.last_reply.created_at),
    },
  }
}

export function toState(record: ConversationStateRecord): ConversationState {
  return {
    conversationId: record.conversation_id,
    lastReadMessageId: record.last_read_message_id,
    unreadCount: record.unread_count,
    mentionCount: record.mention_count,
    notify: record.notify,
    favorite: record.favorite,
  }
}

export function toThreadState(record: ThreadStateRecord): ThreadState {
  return {
    rootId: record.root_id,
    conversationId: record.conversation_id,
    following: record.following,
    lastReadReplyId: record.last_read_reply_id,
    unreadReplies: record.unread_replies,
    mentionCount: record.mention_count,
  }
}

export function toFollowedThread(record: FollowedThreadRecord): FollowedThread {
  return {
    root: toMessage(record.root),
    conversationId: record.conversation_id,
    state: toThreadState(record.state),
    lastReply: record.last_reply && toMessage(record.last_reply),
  }
}

export function toMessagePage(page: WireMessagePage): MessagePage {
  return { items: page.items.map(toMessage), before: page.before, after: page.after }
}

export function toThreadPage(page: WireThreadPage): ThreadPage {
  return { ...toMessagePage(page), root: toMessage(page.root), state: page.state && toThreadState(page.state) }
}

export function toEvent(event: WireEvent, currentUserId: string): ChatEvent {
  switch (event.type) {
    case 'message.created':
    case 'message.updated':
      return { type: event.type, message: toMessage(event.message) }
    case 'message.deleted':
      return { type: event.type, conversationId: event.conversation_id, messageId: event.message_id, threadRootId: event.thread_root_id }
    case 'conversation.changed':
      return { type: event.type, conversation: toConversation(event.conversation, currentUserId) }
    case 'conversation.removed':
      return { type: event.type, conversationId: event.conversation_id }
    case 'categories.changed':
      return { type: event.type, categories: event.categories }
    case 'state.changed':
      return { type: event.type, state: toState(event.state) }
    case 'thread.changed':
      return { type: event.type, state: toThreadState(event.state) }
  }
}

const CODE_BY_STATUS: Partial<Record<number, ChatErrorCode>> = { 403: 'forbidden', 404: 'not_found', 409: 'conflict' }

/**
 * A failed request as the `ChatError` the UI knows. Input that the server refuses (422: an empty name, an empty
 * message) is `conflict`, which the dialogs show as a name problem; only the length limit has its own code. Everything else, a request
 * that did not reach the server included, is `offline`.
 */
export function toChatError(error: unknown): ChatError {
  if (error instanceof ChatError) return error
  if (!(error instanceof ApiProblem)) return new ChatError('offline', 'The server could not be reached.', true)
  const code =
    error.status === 422 ? (error.code === 'chat_message_too_long' ? 'too_long' : 'conflict') : (CODE_BY_STATUS[error.status] ?? 'offline')
  return new ChatError(code, error.detail, [502, 503, 504].includes(error.status))
}
