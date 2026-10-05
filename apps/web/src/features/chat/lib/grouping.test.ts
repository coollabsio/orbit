import { expect, test } from 'bun:test'
import { buildMessageRows } from './grouping'
import { testMessage } from './testMessage'

const MINUTE = 60_000
/** Local time, so "same day" does not depend on the machine's time zone. */
const at = (day: number, hour: number, minute: number) => new Date(2026, 8, day, hour, minute).getTime()
const shape = (rows: ReturnType<typeof buildMessageRows>) =>
  rows.map((row) => (row.type === 'message' ? `${row.groupStart ? 'start' : 'cont'}:${row.message.id}` : row.type))

test('messages of one author within 5 minutes continue the group', () => {
  const rows = buildMessageRows([
    testMessage({ id: 'm1', createdAt: at(1, 10, 0) }),
    testMessage({ id: 'm2', createdAt: at(1, 10, 4) }),
    testMessage({ id: 'm3', createdAt: at(1, 10, 4) + 5 * MINUTE }),
  ])
  expect(shape(rows)).toEqual(['day', 'start:m1', 'cont:m2', 'start:m3'])
})

test('a group ends 10 minutes after its first message, even when each gap is short', () => {
  const rows = buildMessageRows([0, 4, 8, 12, 16].map((minute, index) => testMessage({ id: `m${index}`, createdAt: at(1, 10, minute) })))
  // m3 is 12 minutes after m0 and starts a group; m4 is 4 minutes into that one.
  expect(shape(rows)).toEqual(['day', 'start:m0', 'cont:m1', 'cont:m2', 'start:m3', 'cont:m4'])
})

test('another author starts a group', () => {
  const rows = buildMessageRows([
    testMessage({ id: 'm1', createdAt: at(1, 10, 0) }),
    testMessage({ id: 'm2', createdAt: at(1, 10, 1), authorId: 'u2' }),
    testMessage({ id: 'm3', createdAt: at(1, 10, 2) }),
  ])
  expect(shape(rows)).toEqual(['day', 'start:m1', 'start:m2', 'start:m3'])
})

test('a new day gets a separator and a new group, two minutes apart or not', () => {
  const rows = buildMessageRows([
    testMessage({ id: 'm1', createdAt: at(1, 23, 59) }),
    testMessage({ id: 'm2', createdAt: at(2, 0, 1) }),
  ])
  expect(shape(rows)).toEqual(['day', 'start:m1', 'day', 'start:m2'])
})

test('system rows, deleted roots and "also in channel" replies never join a group', () => {
  const rows = buildMessageRows([
    testMessage({ id: 'm1', createdAt: at(1, 10, 0) }),
    testMessage({ id: 'm2', createdAt: at(1, 10, 1), kind: 'pin' }),
    testMessage({ id: 'm3', createdAt: at(1, 10, 2) }),
    testMessage({ id: 'm4', createdAt: at(1, 10, 3), deleted: true }),
    testMessage({ id: 'm5', createdAt: at(1, 10, 4), threadRootId: 'm1', alsoInChannel: true }),
    testMessage({ id: 'm6', createdAt: at(1, 10, 5) }),
    testMessage({ id: 'm7', createdAt: at(1, 10, 6) }),
  ])
  expect(shape(rows)).toEqual(['day', 'start:m1', 'start:m2', 'start:m3', 'start:m4', 'start:m5', 'start:m6', 'cont:m7'])
})

test('the "New" line sits before the first unread message of another member and breaks the group', () => {
  const messages = [
    testMessage({ id: 'm1', createdAt: at(1, 10, 0), authorId: 'u2' }),
    testMessage({ id: 'm2', createdAt: at(1, 10, 1), authorId: 'u2' }),
    testMessage({ id: 'm3', createdAt: at(1, 10, 2), authorId: 'u1' }),
    testMessage({ id: 'm4', createdAt: at(1, 10, 3), authorId: 'u2' }),
    testMessage({ id: 'm5', createdAt: at(1, 10, 4), authorId: 'u2' }),
  ]
  expect(shape(buildMessageRows(messages, { lastReadMessageId: 'm1', currentUserId: 'u2' }))).toEqual([
    'day', 'start:m1', 'cont:m2', 'new', 'start:m3', 'start:m4', 'cont:m5',
  ])
  // The reader's own message is not new: the line moves to the next message of somebody else.
  expect(shape(buildMessageRows(messages, { lastReadMessageId: 'm2', currentUserId: 'u1' }))).toEqual([
    'day', 'start:m1', 'cont:m2', 'start:m3', 'new', 'start:m4', 'cont:m5',
  ])
})

test('no "New" line when everything is read or no cursor is given; a null cursor means nothing was read', () => {
  const messages = [
    testMessage({ id: 'm1', createdAt: at(1, 10, 0), authorId: 'u2' }),
    testMessage({ id: 'm2', createdAt: at(1, 10, 1), authorId: 'u2' }),
  ]
  expect(shape(buildMessageRows(messages, { lastReadMessageId: 'm2', currentUserId: 'u1' }))).not.toContain('new')
  expect(shape(buildMessageRows(messages, { currentUserId: 'u1' }))).not.toContain('new')
  expect(shape(buildMessageRows(messages, { lastReadMessageId: null, currentUserId: 'u1' }))).toEqual(['day', 'new', 'start:m1', 'cont:m2'])
})

test('an optimistic row keeps its key when the confirmed message replaces it', () => {
  const sending = buildMessageRows([testMessage({ id: '~1-n1', nonce: 'n1', sendState: 'sending' })])
  const confirmed = buildMessageRows([testMessage({ id: 'm9', nonce: 'n1' })])
  expect(sending.at(-1)?.key).toBe('n1')
  expect(confirmed.at(-1)?.key).toBe('n1')
})

test('a reply starts a group, and the next message continues it', () => {
  const rows = buildMessageRows([
    testMessage({ id: 'm1', createdAt: at(1, 10, 0) }),
    testMessage({ id: 'm2', createdAt: at(1, 10, 1), replyToId: 'm0', replyTo: { id: 'm0', authorId: 'u2', body: 'Hi' } }),
    testMessage({ id: 'm3', createdAt: at(1, 10, 2) }),
    // The quoted message was deleted: the line still shows.
    testMessage({ id: 'm4', createdAt: at(1, 10, 3), replyToId: 'm0' }),
  ])
  expect(shape(rows)).toEqual(['day', 'start:m1', 'start:m2', 'cont:m3', 'start:m4'])
})

test('a forward stands alone: it starts a group, and so does the message after it', () => {
  const forwarded = { messageId: 'm0', conversationId: 'c2', authorId: 'u2', createdAt: at(1, 9, 0) }
  const rows = buildMessageRows([
    testMessage({ id: 'm1', createdAt: at(1, 10, 0) }),
    testMessage({ id: 'm2', createdAt: at(1, 10, 1), forwarded }),
    testMessage({ id: 'm3', createdAt: at(1, 10, 2) }),
    testMessage({ id: 'm4', createdAt: at(1, 10, 3) }),
  ])
  expect(shape(rows)).toEqual(['day', 'start:m1', 'start:m2', 'start:m3', 'cont:m4'])
})
