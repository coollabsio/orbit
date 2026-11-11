import { expect, test } from 'bun:test'
import type { Conversation, ConversationState, FollowedThread } from '../api/types'
import { buildSidebarSections, chatBadges, conversationBadge, dmTitle, dropBefore, inChannelGroup, isSelfDm, unjoinedChannelCount, unreadGroups } from './sidebar'
import { testMessage } from './testMessage'

const conversation = (id: string, overrides: Partial<Conversation> = {}): Conversation => ({
  id, kind: 'public', name: id, topic: '', categoryId: null, position: 0, memberIds: ['u1'], isMember: true,
  isDefault: false, selfDm: false, archived: false, createdBy: 'u1', createdAt: 0, lastMessageAt: null, ...overrides,
})
const state = (conversationId: string, overrides: Partial<ConversationState> = {}): ConversationState => ({
  conversationId, lastReadMessageId: null, unreadCount: 0, mentionCount: 0, notify: 'mentions', favorite: false, ...overrides,
})

test('the badge follows the notification table', () => {
  const channel = conversation('c1')
  expect(conversationBadge(channel, state('c1', { notify: 'all', unreadCount: 4, mentionCount: 1 }))).toEqual({
    bold: true, count: 4, marker: false, inUnreads: true,
  })
  expect(conversationBadge(channel, state('c1', { notify: 'mentions', unreadCount: 4, mentionCount: 0 }))).toEqual({
    bold: true, count: 0, marker: true, inUnreads: true,
  })
  expect(conversationBadge(channel, state('c1', { notify: 'mentions', unreadCount: 4, mentionCount: 2 }))).toEqual({
    bold: true, count: 2, marker: false, inUnreads: true,
  })
  expect(conversationBadge(channel, state('c1', { notify: 'muted', unreadCount: 4, mentionCount: 1 }))).toEqual({
    bold: false, count: 1, marker: false, inUnreads: false,
  })
  expect(conversationBadge(channel, state('c1'))).toEqual({ bold: false, count: 0, marker: false, inUnreads: false })
  // A channel the user has not joined never shows as unread.
  expect(conversationBadge(conversation('c2', { isMember: false }), state('c2', { unreadCount: 3, mentionCount: 3 })).count).toBe(0)
})

test('the nav badge is the sum of the count badges plus unread followed threads', () => {
  const conversations = [conversation('c1'), conversation('c2'), conversation('d1', { kind: 'dm' })]
  const states = [
    state('c1', { unreadCount: 5, mentionCount: 2 }),
    state('c2', { notify: 'muted', unreadCount: 9, mentionCount: 1 }),
    state('d1', { notify: 'all', unreadCount: 3 }),
  ]
  const thread = (id: string, unreadReplies: number): FollowedThread => ({
    root: testMessage({ id }), conversationId: 'c1', lastReply: null,
    state: { rootId: id, conversationId: 'c1', following: true, lastReadReplyId: null, unreadReplies, mentionCount: 0 },
  })
  expect(chatBadges(conversations, states, [thread('m1', 4), thread('m2', 0), thread('m3', 1)])).toEqual({ unreads: 6, threads: 2, total: 8, unread: true })
})

test('Unreads lists unread conversations and unread threads, followed or not, of joined, not muted conversations, newest first', () => {
  const conversations = [
    conversation('c1', { lastMessageAt: 10 }),
    conversation('muted', { lastMessageAt: 50 }),
    conversation('old', { archived: true, lastMessageAt: 60 }),
    conversation('browse', { isMember: false, lastMessageAt: 70 }),
    conversation('d1', { kind: 'dm', lastMessageAt: 30 }),
  ]
  const states = [
    state('c1', { unreadCount: 2 }),
    state('muted', { notify: 'muted', unreadCount: 9 }),
    state('old', { unreadCount: 1 }),
    state('browse', { unreadCount: 1 }),
    state('d1', { notify: 'all' }),
  ]
  const thread = (id: string, conversationId: string, unreadReplies: number, lastReplyAt: number, following = true): FollowedThread => ({
    root: testMessage({ id, conversationId, lastReplyAt }), conversationId, lastReply: null,
    state: { rootId: id, conversationId, following, lastReadReplyId: null, unreadReplies, mentionCount: 0 },
  })
  const threads = [
    thread('t-d1', 'd1', 1, 40),
    thread('t-c1', 'c1', 3, 5),
    thread('t-read', 'c1', 0, 80),
    thread('t-unfollowed', 'c1', 2, 80, false),
    thread('t-muted', 'muted', 4, 90),
    thread('t-old', 'old', 4, 90),
    thread('t-browse', 'browse', 4, 90),
    thread('t-gone', 'missing', 4, 90),
  ]
  const groups = unreadGroups(conversations, states, threads)
  expect(groups.map((group) => (group.kind === 'thread' ? group.thread.root.id : group.conversation.id))).toEqual(['t-unfollowed', 't-d1', 'c1', 't-c1'])
  // A thread knows the state of its conversation only when that conversation has a group too.
  const channelOf = (rootId: string) => groups.flatMap((group) => (group.kind === 'thread' && group.thread.root.id === rootId ? [group.channel] : []))[0]
  expect(channelOf('t-c1')?.conversationId).toBe('c1')
  expect(channelOf('t-d1')).toBeNull()
})

test('a reply also sent to the channel shows in the channel group only while that group shows it', () => {
  const reply = testMessage({ id: 'm5', alsoInChannel: true })
  expect(inChannelGroup(reply, state('c1', { lastReadMessageId: 'm4', unreadCount: 1 }))).toBe(true)
  expect(inChannelGroup(reply, state('c1', { lastReadMessageId: null, unreadCount: 1 }))).toBe(true)
  expect(inChannelGroup(reply, state('c1', { lastReadMessageId: 'm5' }))).toBe(false)
  expect(inChannelGroup(reply, null)).toBe(false)
  expect(inChannelGroup(testMessage({ id: 'm5' }), state('c1', { lastReadMessageId: 'm4', unreadCount: 1 }))).toBe(false)
})

test('the nav item is unread without a count when a channel only has messages without mentions', () => {
  const conversations = [conversation('c1'), conversation('c2')]
  const unread = [state('c1', { unreadCount: 5 }), state('c2', { notify: 'muted', unreadCount: 9 })]
  expect(chatBadges(conversations, unread, [])).toEqual({ unreads: 0, threads: 0, total: 0, unread: true })
  // A muted conversation alone does not count as unread.
  expect(chatBadges(conversations, [state('c2', { notify: 'muted', unreadCount: 9 })], []).unread).toBe(false)
})

test('sidebar sections: favorites, categories by position, Channels, then DMs by last activity', () => {
  const categories = [
    { id: 'g2', name: 'Company', position: 1 },
    { id: 'g1', name: 'Product', position: 0 },
  ]
  const conversations = [
    conversation('general', { position: 0 }),
    conversation('zeta', { categoryId: 'g1', position: 0 }),
    conversation('alpha', { categoryId: 'g1', position: 1 }),
    conversation('starred', { categoryId: 'g1', position: 2 }),
    conversation('browse-only', { categoryId: 'g1', isMember: false }),
    conversation('old', { categoryId: 'g2', archived: true }),
    conversation('d-old', { kind: 'dm', name: '', lastMessageAt: 10 }),
    conversation('d-new', { kind: 'dm', name: '', lastMessageAt: 20 }),
  ]
  const sections = buildSidebarSections(conversations, categories, [state('starred', { favorite: true })])
  expect(sections.map((section) => [section.title, section.conversations.map((item) => item.id)])).toEqual([
    ['Favorites', ['starred']],
    ['Product', ['zeta', 'alpha']],
    ['Company', []],
    ['Channels', ['general']],
    ['Direct messages', ['d-new', 'd-old']],
  ])
})

test('the DM with yourself comes first among the direct messages', () => {
  const conversations = [
    conversation('d-new', { kind: 'dm', name: '', memberIds: ['u1', 'u2'], lastMessageAt: 20 }),
    conversation('notes', { kind: 'dm', name: '', memberIds: ['u1'], selfDm: true, lastMessageAt: 5 }),
    conversation('d-old', { kind: 'dm', name: '', memberIds: ['u1', 'u3'], lastMessageAt: 10 }),
  ]
  const dms = buildSidebarSections(conversations, [], []).find((section) => section.kind === 'dms')
  expect(dms?.conversations.map((item) => item.id)).toEqual(['notes', 'd-new', 'd-old'])
  expect(isSelfDm(conversations[1])).toBe(true)
  expect(isSelfDm(conversations[0])).toBe(false)
  // a 1:1 DM whose other member left the workspace is not the user's notes
  expect(isSelfDm(conversation('d-left', { kind: 'dm', memberIds: ['u1'] }))).toBe(false)
})

test('a DM is titled by its other members', () => {
  const people = [{ id: 'u1', name: 'Me' }, { id: 'u2', name: 'Ada' }, { id: 'u3', name: 'Grace' }]
  expect(dmTitle(conversation('d1', { kind: 'dm', memberIds: ['u1', 'u2'] }), people, 'u1')).toBe('Ada')
  expect(dmTitle(conversation('d2', { kind: 'dm', memberIds: ['u1', 'u2', 'u3'] }), people, 'u1')).toBe('Ada, Grace')
  expect(dmTitle(conversation('d3', { kind: 'dm', memberIds: ['u1'], selfDm: true }), people, 'u1')).toBe('Me (you)')
  expect(dmTitle(conversation('d4', { kind: 'dm', memberIds: ['u1'] }), people, 'u1')).toBe('Former member')
})

test('a dropped channel goes before the row, or before the one after it', () => {
  const channels = ['a', 'b', 'c'].map((id) => ({ id }) as Conversation)
  expect(dropBefore(channels, 'x', 'b', 'before')).toBe('b')
  expect(dropBefore(channels, 'x', 'b', 'after')).toBe('c')
  expect(dropBefore(channels, 'x', 'c', 'after')).toBeNull()
  // Inside its own section the dragged channel does not count.
  expect(dropBefore(channels, 'b', 'a', 'after')).toBe('c')
  expect(dropBefore(channels, 'c', 'b', 'after')).toBeNull()
})

test('the channels left to join are public, not joined and not archived', () => {
  const conversations = [
    conversation('joined'),
    conversation('open', { isMember: false }),
    conversation('old', { isMember: false, archived: true }),
    conversation('secret', { kind: 'private', isMember: false }),
  ]
  expect(unjoinedChannelCount(conversations)).toBe(1)
})
