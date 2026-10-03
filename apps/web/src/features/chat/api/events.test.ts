import { expect, test } from 'bun:test'
import { QueryClient } from '@tanstack/react-query'
import { testMessage } from '../lib/testMessage'
import { applyChatEvent, type MessagePages, type ThreadPages, updateOutbox } from './events'
import { chatKeys } from './keys'
import type { Category, Conversation, ConversationState, FollowedThread, Message, ThreadState } from './types'

const W = 'w1'

const conversation = (id: string, overrides: Partial<Conversation> = {}): Conversation => ({
  id, kind: 'public', name: id, topic: '', categoryId: null, position: 0, memberIds: ['u1'], isMember: true,
  isDefault: false, selfDm: false, archived: false, createdBy: 'u1', createdAt: 0, lastMessageAt: 100, ...overrides,
})
const state = (conversationId: string, unreadCount = 0): ConversationState => ({
  conversationId, lastReadMessageId: null, unreadCount, mentionCount: 0, notify: 'mentions', favorite: false,
})
const threadState = (rootId: string, overrides: Partial<ThreadState> = {}): ThreadState => ({
  rootId, conversationId: 'c1', following: true, lastReadReplyId: null, unreadReplies: 0, mentionCount: 0, ...overrides,
})
const pages = (items: Message[], cursors: { before?: string | null; after?: string | null } = {}): MessagePages => ({
  pages: [{ items, before: cursors.before ?? null, after: cursors.after ?? null }],
  pageParams: [{}],
})

function setup() {
  const client = new QueryClient()
  const root = testMessage({ id: 'm1', createdAt: 100 })
  client.setQueryData(chatKeys.conversations(W), [conversation('c1'), conversation('c2')])
  client.setQueryData(chatKeys.states(W), [state('c1'), state('c2')])
  client.setQueryData(chatKeys.messages(W, 'c1'), pages([root, testMessage({ id: 'm2', createdAt: 110 })]))
  const ids = (key: readonly unknown[] = chatKeys.messages(W, 'c1')) =>
    client.getQueryData<MessagePages>(key)!.pages.flatMap((page) => page.items.map((item) => item.id))
  const find = (id: string) => client.getQueryData<MessagePages>(chatKeys.messages(W, 'c1'))!.pages[0].items.find((item) => item.id === id)
  return { client, root, ids, find }
}

test('message.created appends to the newest page and moves the conversation’s last activity', () => {
  const { client, ids } = setup()
  applyChatEvent(client, W, { type: 'message.created', message: testMessage({ id: 'm3', createdAt: 120 }) })
  expect(ids()).toEqual(['m1', 'm2', 'm3'])
  expect(client.getQueryData<Conversation[]>(chatKeys.conversations(W))![0].lastMessageAt).toBe(120)
  // The same event again (the mutation result after the echo) changes nothing.
  applyChatEvent(client, W, { type: 'message.created', message: testMessage({ id: 'm3', createdAt: 120 }) })
  expect(ids()).toEqual(['m1', 'm2', 'm3'])
})

test('message.created is not appended to a window that does not reach the newest message', () => {
  const { client, ids } = setup()
  const around = chatKeys.messages(W, 'c1', 'm1')
  client.setQueryData(around, pages([testMessage({ id: 'm1' })], { after: 'm1' }))
  applyChatEvent(client, W, { type: 'message.created', message: testMessage({ id: 'm3' }) })
  expect(ids(around)).toEqual(['m1'])
  expect(ids()).toEqual(['m1', 'm2', 'm3'])
})

test('the confirmed message takes the place of the row in the outbox with its nonce', () => {
  const { client, ids } = setup()
  const outbox = () => client.getQueryData<Message[]>(chatKeys.outbox(W))?.map((message) => message.id)
  updateOutbox(client, W, () => [
    testMessage({ id: '~001-n1', nonce: 'n1', sendState: 'sending' }),
    testMessage({ id: '~002-n2', nonce: 'n2', sendState: 'failed' }),
  ])
  applyChatEvent(client, W, { type: 'message.created', message: testMessage({ id: 'm5', nonce: 'n2', body: 'second' }) })
  expect(ids()).toEqual(['m1', 'm2', 'm5'])
  expect(outbox()).toEqual(['~001-n1'])
  // A resync asks the lists again and leaves the outbox alone.
  applyChatEvent(client, W, { type: 'resync' })
  expect(outbox()).toEqual(['~001-n1'])
})

test('an event that comes while a list loads is applied again when the load ends', async () => {
  const client = new QueryClient()
  let release = () => {}
  // The server read the page before the message was there.
  const loaded = client.fetchInfiniteQuery({
    queryKey: chatKeys.messages(W, 'c1'),
    initialPageParam: {},
    queryFn: async () => {
      await new Promise<void>((resolve) => (release = resolve))
      return { items: [testMessage({ id: 'm1' })], before: null, after: null }
    },
  })
  await Promise.resolve()
  // No data yet: the events themselves change nothing.
  applyChatEvent(client, W, { type: 'message.created', message: testMessage({ id: 'm2' }) })
  applyChatEvent(client, W, { type: 'message.updated', message: testMessage({ id: 'm2', body: 'edited' }) })
  release()
  await loaded
  await new Promise((resolve) => setTimeout(resolve, 10))
  const items = client.getQueryData<MessagePages>(chatKeys.messages(W, 'c1'))!.pages[0].items
  expect(items.map((message) => [message.id, message.body])).toEqual([['m1', testMessage({ id: 'm1' }).body], ['m2', 'edited']])
})

test('a reply goes to its thread and updates the root’s summary in the conversation list, once', () => {
  const { client, root, ids, find } = setup()
  const threadKey = chatKeys.thread(W, 'm1')
  client.setQueryData<ThreadPages>(threadKey, { pages: [{ items: [], before: null, after: null, root, state: null }], pageParams: [{}] })
  client.setQueryData<FollowedThread[]>(chatKeys.followedThreads(W), [
    { root, conversationId: 'c1', state: threadState('m1'), lastReply: null },
  ])
  const reply = testMessage({ id: 'm7', threadRootId: 'm1', authorId: 'u2', createdAt: 200 })
  applyChatEvent(client, W, { type: 'message.created', message: reply })
  applyChatEvent(client, W, { type: 'message.created', message: reply })

  expect(ids()).toEqual(['m1', 'm2'])
  expect(find('m1')).toMatchObject({ replyCount: 1, lastReplyAt: 200, replyUserIds: ['u2'] })
  const thread = client.getQueryData<ThreadPages>(threadKey)!.pages[0]
  expect(thread.items.map((item) => item.id)).toEqual(['m7'])
  expect(thread.root.replyCount).toBe(1)
  const followed = client.getQueryData<FollowedThread[]>(chatKeys.followedThreads(W))![0]
  expect(followed.lastReply?.id).toBe('m7')
  expect(followed.root.replyCount).toBe(1)
  // A thread reply is not conversation activity.
  expect(client.getQueryData<Conversation[]>(chatKeys.conversations(W))![0].lastMessageAt).toBe(100)

  // The root's own update is authoritative, whichever order the two events arrive in.
  applyChatEvent(client, W, { type: 'message.updated', message: { ...root, replyCount: 1, lastReplyAt: 200, replyUserIds: ['u2'] } })
  expect(find('m1')?.replyCount).toBe(1)
})

test('a root that gets its first reply joins the conversation’s Threads list, newest reply first', () => {
  const { client, root } = setup()
  const threadsKey = chatKeys.threads(W, 'c1')
  const older = testMessage({ id: 'm0', createdAt: 50, replyCount: 2, lastReplyAt: 150 })
  client.setQueryData<Message[]>(threadsKey, [older])
  const threads = () => client.getQueryData<Message[]>(threadsKey)!.map((item) => [item.id, item.replyCount])

  // The root's update after the first reply (another member's reply, as the socket sends it).
  const replied = { ...root, replyCount: 1, lastReplyAt: 200, replyUserIds: ['u2'] }
  applyChatEvent(client, W, { type: 'message.updated', message: replied })
  expect(threads()).toEqual([['m1', 1], ['m0', 2]])
  // Once more (the response after the socket) changes nothing.
  applyChatEvent(client, W, { type: 'message.updated', message: replied })
  expect(threads()).toEqual([['m1', 1], ['m0', 2]])

  // A reply to the older thread moves it to the top.
  applyChatEvent(client, W, { type: 'message.updated', message: { ...older, replyCount: 3, lastReplyAt: 300 } })
  expect(threads()).toEqual([['m0', 3], ['m1', 1]])
})

test('a reply puts its root into the Threads list from the copy in the message list', () => {
  const { client } = setup()
  const threadsKey = chatKeys.threads(W, 'c1')
  client.setQueryData<Message[]>(threadsKey, [])
  applyChatEvent(client, W, { type: 'message.created', message: testMessage({ id: 'm7', threadRootId: 'm1', authorId: 'u2', createdAt: 200 }) })
  expect(client.getQueryData<Message[]>(threadsKey)!.map((item) => [item.id, item.replyCount])).toEqual([['m1', 1]])
})

test('an event that comes while the Threads list loads is applied again when the load ends', async () => {
  const { client, root } = setup()
  let release = () => {}
  // The server read the list before the reply was there.
  const loaded = client.fetchQuery({
    queryKey: chatKeys.threads(W, 'c1'),
    queryFn: async () => {
      await new Promise<void>((resolve) => (release = resolve))
      return [] as Message[]
    },
  })
  await Promise.resolve()
  applyChatEvent(client, W, { type: 'message.updated', message: { ...root, replyCount: 1, lastReplyAt: 200 } })
  release()
  await loaded
  await new Promise((resolve) => setTimeout(resolve, 10))
  expect(client.getQueryData<Message[]>(chatKeys.threads(W, 'c1'))!.map((item) => item.id)).toEqual(['m1'])
})

test('a reply to a followed thread that the Threads view does not list yet refetches that list', () => {
  const { client, root } = setup()
  client.setQueryData<ThreadPages>(chatKeys.thread(W, 'm1'), {
    pages: [{ items: [], before: null, after: null, root, state: threadState('m1') }],
    pageParams: [{}],
  })
  client.setQueryData<FollowedThread[]>(chatKeys.followedThreads(W), [])
  applyChatEvent(client, W, { type: 'message.created', message: testMessage({ id: 'm7', threadRootId: 'm1', createdAt: 200 }) })
  expect(client.getQueryState(chatKeys.followedThreads(W))?.isInvalidated).toBe(true)
})

test('a reply to a thread the user does not follow leaves the Threads view alone', () => {
  const { client } = setup()
  client.setQueryData<FollowedThread[]>(chatKeys.followedThreads(W), [])
  applyChatEvent(client, W, { type: 'message.created', message: testMessage({ id: 'm7', threadRootId: 'm1', createdAt: 200 }) })
  expect(client.getQueryState(chatKeys.followedThreads(W))?.isInvalidated).toBe(false)
})

test('an "also in channel" reply shows in the main list as well', () => {
  const { client, ids } = setup()
  applyChatEvent(client, W, {
    type: 'message.created',
    message: testMessage({ id: 'm7', threadRootId: 'm1', alsoInChannel: true, createdAt: 200 }),
  })
  expect(ids()).toEqual(['m1', 'm2', 'm7'])
})

test('message.updated replaces the message and keeps the pins list in step', () => {
  const { client, find } = setup()
  client.setQueryData<Message[]>(chatKeys.pins(W, 'c1'), [])
  const pinned = testMessage({ id: 'm2', body: 'edited', editedAt: 5, pinned: true })
  applyChatEvent(client, W, { type: 'message.updated', message: pinned })
  expect(find('m2')).toMatchObject({ body: 'edited', pinned: true })
  expect(client.getQueryData<Message[]>(chatKeys.pins(W, 'c1'))!.map((item) => item.id)).toEqual(['m2'])
  applyChatEvent(client, W, { type: 'message.updated', message: { ...pinned, pinned: false } })
  expect(client.getQueryData<Message[]>(chatKeys.pins(W, 'c1'))).toEqual([])
})

test('message.deleted removes the message from the list, the pins and its thread', () => {
  const { client, root, ids } = setup()
  client.setQueryData<Message[]>(chatKeys.pins(W, 'c1'), [testMessage({ id: 'm2', pinned: true })])
  const reply = testMessage({ id: 'm7', threadRootId: 'm1' })
  client.setQueryData<ThreadPages>(chatKeys.thread(W, 'm1'), {
    pages: [{ items: [reply], before: null, after: null, root, state: null }],
    pageParams: [{}],
  })
  applyChatEvent(client, W, { type: 'message.deleted', conversationId: 'c1', messageId: 'm2', threadRootId: null })
  applyChatEvent(client, W, { type: 'message.deleted', conversationId: 'c1', messageId: 'm7', threadRootId: 'm1' })
  expect(ids()).toEqual(['m1'])
  expect(client.getQueryData<Message[]>(chatKeys.pins(W, 'c1'))).toEqual([])
  expect(client.getQueryData<ThreadPages>(chatKeys.thread(W, 'm1'))!.pages[0].items).toEqual([])
})

test('conversation.changed replaces a known conversation and adds a new one', () => {
  const { client } = setup()
  applyChatEvent(client, W, { type: 'conversation.changed', conversation: conversation('c2', { topic: 'new topic' }) })
  applyChatEvent(client, W, { type: 'conversation.changed', conversation: conversation('c3') })
  const list = client.getQueryData<Conversation[]>(chatKeys.conversations(W))!
  expect(list.map((item) => item.id)).toEqual(['c1', 'c2', 'c3'])
  expect(list[1].topic).toBe('new topic')
})

test('conversation.removed drops the conversation, its state, its messages and its followed threads', () => {
  const { client, root } = setup()
  client.setQueryData<FollowedThread[]>(chatKeys.followedThreads(W), [
    { root, conversationId: 'c1', state: threadState('m1'), lastReply: null },
  ])
  applyChatEvent(client, W, { type: 'conversation.removed', conversationId: 'c1' })
  expect(client.getQueryData<Conversation[]>(chatKeys.conversations(W))!.map((item) => item.id)).toEqual(['c2'])
  expect(client.getQueryData<ConversationState[]>(chatKeys.states(W))!.map((item) => item.conversationId)).toEqual(['c2'])
  expect(client.getQueryData(chatKeys.messages(W, 'c1'))).toBeUndefined()
  expect(client.getQueryData<FollowedThread[]>(chatKeys.followedThreads(W))).toEqual([])
})

test('categories.changed and state.changed write the new values', () => {
  const { client } = setup()
  const categories: Category[] = [{ id: 'g1', name: 'Product', position: 0 }]
  applyChatEvent(client, W, { type: 'categories.changed', categories })
  expect(client.getQueryData<Category[]>(chatKeys.categories(W))).toEqual(categories)

  applyChatEvent(client, W, { type: 'state.changed', state: state('c2', 4) })
  applyChatEvent(client, W, { type: 'state.changed', state: state('c9', 1) })
  expect(client.getQueryData<ConversationState[]>(chatKeys.states(W))!.map((item) => [item.conversationId, item.unreadCount])).toEqual([
    ['c1', 0],
    ['c2', 4],
    ['c9', 1],
  ])
})

test('thread.changed updates the thread and the followed list; unfollowing removes it there', () => {
  const { client, root } = setup()
  const threadKey = chatKeys.thread(W, 'm1')
  const other = testMessage({ id: 'm0', createdAt: 50 })
  client.setQueryData<ThreadPages>(threadKey, { pages: [{ items: [], before: null, after: null, root, state: null }], pageParams: [{}] })
  client.setQueryData<FollowedThread[]>(chatKeys.followedThreads(W), [
    { root: other, conversationId: 'c1', state: threadState('m0'), lastReply: null },
    { root, conversationId: 'c1', state: threadState('m1'), lastReply: null },
  ])
  applyChatEvent(client, W, { type: 'thread.changed', state: threadState('m1', { unreadReplies: 2 }) })
  expect(client.getQueryData<ThreadPages>(threadKey)!.pages[0].state?.unreadReplies).toBe(2)
  // Unread threads sort first.
  expect(client.getQueryData<FollowedThread[]>(chatKeys.followedThreads(W))!.map((item) => [item.root.id, item.state.unreadReplies])).toEqual([
    ['m1', 2],
    ['m0', 0],
  ])

  applyChatEvent(client, W, { type: 'thread.changed', state: threadState('m1', { following: false }) })
  expect(client.getQueryData<FollowedThread[]>(chatKeys.followedThreads(W))!.map((item) => item.root.id)).toEqual(['m0'])
  expect(client.getQueryData<ThreadPages>(threadKey)!.pages[0].state?.following).toBe(false)

  // A thread that is newly followed is not in the list: it has to be fetched.
  applyChatEvent(client, W, { type: 'thread.changed', state: threadState('m9') })
  expect(client.getQueryState(chatKeys.followedThreads(W))?.isInvalidated).toBe(true)
})

test('resync invalidates every chat query of the workspace and nothing else', () => {
  const { client } = setup()
  client.setQueryData(['workspace', W, 'members'], [])
  client.setQueryData(chatKeys.conversations('w2'), [])
  applyChatEvent(client, W, { type: 'resync' })
  expect(client.getQueryState(chatKeys.conversations(W))?.isInvalidated).toBe(true)
  expect(client.getQueryState(chatKeys.messages(W, 'c1'))?.isInvalidated).toBe(true)
  expect(client.getQueryState(['workspace', W, 'members'])?.isInvalidated).toBe(false)
  expect(client.getQueryState(chatKeys.conversations('w2'))?.isInvalidated).toBe(false)
})

test('typing, presence and connection events leave the cache alone', () => {
  const { client, ids } = setup()
  applyChatEvent(client, W, { type: 'typing', conversationId: 'c1', threadRootId: null, userId: 'u2' })
  applyChatEvent(client, W, { type: 'presence', userId: 'u2', online: true })
  applyChatEvent(client, W, { type: 'connection', status: 'reconnecting' })
  expect(ids()).toEqual(['m1', 'm2'])
})
