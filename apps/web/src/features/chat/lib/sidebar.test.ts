import { expect, test } from 'bun:test'
import type { Conversation, ConversationState, FollowedThread } from '../api/types'
import { buildSidebarSections, chatBadges, conversationBadge, dmTitle, dropBefore, unreadConversations } from './sidebar'
import { testMessage } from './testMessage'

const conversation = (id: string, overrides: Partial<Conversation> = {}): Conversation => ({
  id, kind: 'public', name: id, topic: '', categoryId: null, position: 0, memberIds: ['u1'], isMember: true,
  isDefault: false, archived: false, createdBy: 'u1', createdAt: 0, lastMessageAt: null, ...overrides,
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
  expect(chatBadges(conversations, states, [thread('m1', 4), thread('m2', 0), thread('m3', 1)])).toEqual({ unreads: 6, threads: 2, total: 8 })
  expect(unreadConversations(conversations, states).map((item) => item.conversation.id).sort()).toEqual(['c1', 'd1'])
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

test('a DM is titled by its other members', () => {
  const people = [{ id: 'u1', name: 'Me' }, { id: 'u2', name: 'Ada' }, { id: 'u3', name: 'Grace' }]
  expect(dmTitle(conversation('d1', { kind: 'dm', memberIds: ['u1', 'u2'] }), people, 'u1')).toBe('Ada')
  expect(dmTitle(conversation('d2', { kind: 'dm', memberIds: ['u1', 'u2', 'u3'] }), people, 'u1')).toBe('Ada, Grace')
  expect(dmTitle(conversation('d3', { kind: 'dm', memberIds: ['u1'] }), people, 'u1')).toBe('Me (you)')
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
