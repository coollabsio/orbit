import { expect, test } from 'bun:test'
import type { ChatEvent as WireEvent, ConversationRecord, MessageRecord } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { ChatError } from './types'
import { toChatError, toEvent } from './wire'

const message: MessageRecord = {
  id: 'm2',
  conversation_id: 'c1',
  thread_root_id: 'm1',
  kind: 'message',
  author_id: 'u2',
  body: '<@u1> done',
  mentions: { user_ids: ['u1'], channel: false, here: false },
  created_at: '2026-10-02T08:00:00.250Z',
  edited_at: '2026-10-02T08:01:00.000Z',
  deleted: false,
  attachments: [
    { id: 'f1', url: '/api/v1/workspaces/w1/chat/files/f1', file_name: 'plan.png', mime_type: 'image/png', size_bytes: 68, width: 640, height: 400 },
    { id: 'f2', url: '/api/v1/workspaces/w1/chat/files/f2', file_name: 'notes.txt', mime_type: 'text/plain', size_bytes: 12, width: null, height: null },
  ],
  reactions: [{ emoji: '👍', user_ids: ['u1', 'u2'] }],
  pinned: true,
  also_in_channel: true,
  nonce: 'n1',
  reply_count: 2,
  last_reply_at: '2026-10-02T08:02:00.000Z',
  reply_user_ids: ['u1'],
  last_reply: { author_id: 'u1', body: 'Thanks', created_at: '2026-10-02T08:02:00.000Z' },
}

const conversation: ConversationRecord = {
  id: 'c1',
  kind: 'private',
  name: 'plans',
  topic: '',
  category_id: null,
  position: 3,
  member_ids: ['u1', 'u2'],
  is_default: false,
  self_dm: false,
  archived: false,
  created_by: 'u2',
  created_at: '2026-10-01T00:00:00.000Z',
  last_message_at: null,
}

test('a wire message becomes a chat message with millisecond times', () => {
  expect(toEvent({ type: 'message.created', message }, 'u1')).toEqual({
    type: 'message.created',
    message: {
      id: 'm2',
      conversationId: 'c1',
      threadRootId: 'm1',
      kind: 'message',
      authorId: 'u2',
      body: '<@u1> done',
      mentions: { userIds: ['u1'], channel: false, here: false },
      createdAt: Date.UTC(2026, 9, 2, 8, 0, 0, 250),
      editedAt: Date.UTC(2026, 9, 2, 8, 1),
      deleted: false,
      attachments: [
        { id: 'f1', fileName: 'plan.png', mimeType: 'image/png', fileSize: 68, url: '/api/v1/workspaces/w1/chat/files/f1', width: 640, height: 400 },
        { id: 'f2', fileName: 'notes.txt', mimeType: 'text/plain', fileSize: 12, url: '/api/v1/workspaces/w1/chat/files/f2' },
      ],
      reactions: [{ emoji: '👍', userIds: ['u1', 'u2'] }],
      pinned: true,
      alsoInChannel: true,
      nonce: 'n1',
      replyCount: 2,
      lastReplyAt: Date.UTC(2026, 9, 2, 8, 2),
      replyUserIds: ['u1'],
      lastReply: { authorId: 'u1', body: 'Thanks', createdAt: Date.UTC(2026, 9, 2, 8, 2) },
    },
  })
})

test('membership of a conversation depends on who reads the event', () => {
  const event: WireEvent = { type: 'conversation.changed', conversation }
  const seenBy = (userId: string) => {
    const mapped = toEvent(event, userId)
    return mapped.type === 'conversation.changed' && mapped.conversation.isMember
  }
  expect(seenBy('u1')).toBe(true)
  expect(seenBy('u3')).toBe(false)
})

test('the other events keep their ids', () => {
  expect(toEvent({ type: 'message.deleted', conversation_id: 'c1', message_id: 'm2', thread_root_id: 'm1' }, 'u1')).toEqual({
    type: 'message.deleted',
    conversationId: 'c1',
    messageId: 'm2',
    threadRootId: 'm1',
  })
  expect(
    toEvent({ type: 'thread.changed', state: { root_id: 'm1', conversation_id: 'c1', following: true, last_read_reply_id: null, unread_replies: 2, mention_count: 1 } }, 'u1'),
  ).toEqual({ type: 'thread.changed', state: { rootId: 'm1', conversationId: 'c1', following: true, lastReadReplyId: null, unreadReplies: 2, mentionCount: 1 } })
})

test('a failed request becomes the chat error the UI knows', () => {
  const problem = (status: number, code: string) =>
    toChatError(new ApiProblem({ type: 'about:blank', title: 'x', status, code, detail: 'Why it failed.', instance: '/x', request_id: 'r' }))
  expect(problem(404, 'chat_not_found').code).toBe('not_found')
  expect(problem(403, 'chat_forbidden').code).toBe('forbidden')
  expect(problem(409, 'chat_conflict').code).toBe('conflict')
  expect(problem(422, 'chat_message_too_long').code).toBe('too_long')
  // An empty channel name or message: the dialogs treat it as `conflict`.
  expect(problem(422, 'validation_failed').code).toBe('conflict')
  expect(problem(500, 'internal_error').code).toBe('offline')
  expect(problem(403, 'chat_forbidden').message).toBe('Why it failed.')
  expect(toChatError(new TypeError('Failed to fetch')).code).toBe('offline')
  const known = new ChatError('upload_failed', 'x')
  expect(toChatError(known)).toBe(known)
})
