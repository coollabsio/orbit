import type { Message } from '../api/types'

/** A plain message for tests; override what the test is about. */
export function testMessage(overrides: Partial<Message> & { id: string }): Message {
  return {
    conversationId: 'c1',
    threadRootId: null,
    kind: 'message',
    authorId: 'u1',
    body: 'text',
    mentions: { userIds: [], channel: false, here: false },
    createdAt: 0,
    editedAt: null,
    deleted: false,
    attachments: [],
    reactions: [],
    pinned: false,
    alsoInChannel: false,
    nonce: null,
    replyCount: 0,
    lastReplyAt: null,
    replyUserIds: [],
    lastReply: null,
    ...overrides,
  }
}
