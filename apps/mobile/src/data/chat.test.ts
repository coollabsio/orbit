import { expect, test } from 'bun:test'

import {
  buildMessageRows,
  channelNameFinal,
  channelNameInput,
  chatSections,
  chatBadge,
  conversationActions,
  conversationPermissions,
  conversationTitle,
  customEmojiName,
  decodeMentions,
  emojiGroups,
  fileSize,
  hasReaction,
  leadToFirstMatch,
  messageActions,
  messageablePeople,
  type MessagePages,
  pickablePeople,
  quickReactions,
  removeMessage,
  searchEmoji,
  sortFollowedThreads,
  splitByRanges,
  type ThreadPages,
  toggleReaction,
  unreadConversations,
  unreadMessages,
  unreadThreadCount,
  upsertMessage,
  upsertThreadMessage,
} from './chat'
import type { EmojibaseData, FollowedThreadRecord, ThreadPage } from './chat-api'
import type { ConversationRecord, ConversationStateRecord, MemberRecord, MessageRecord } from './orbit'

const conversation = (id: string, fields: Partial<ConversationRecord>): ConversationRecord => ({
  id,
  name: id,
  kind: 'public',
  archived: false,
  category_id: null,
  created_at: '2026-10-01T00:00:00Z',
  created_by: 'me',
  is_default: false,
  last_message_at: null,
  member_ids: ['me'],
  position: 0,
  self_dm: false,
  topic: '',
  ...fields,
})

const state = (conversation_id: string, fields: Partial<ConversationStateRecord>): ConversationStateRecord => ({
  conversation_id,
  favorite: false,
  last_read_message_id: null,
  mention_count: 0,
  notify: 'all',
  unread_count: 0,
  ...fields,
})

const message = (id: string, fields: Partial<MessageRecord>): MessageRecord =>
  ({
    id,
    author_id: 'ada',
    body: id,
    conversation_id: 'c1',
    created_at: '2026-10-23T10:00:00Z',
    kind: 'message',
    deleted: false,
    also_in_channel: false,
    forwarded: null,
    reply_to_id: null,
    thread_root_id: null,
    nonce: null,
    reactions: [],
    reply_count: 0,
    last_reply_at: null,
    ...fields,
  }) as MessageRecord

const CONVERSATIONS = [
  conversation('general', { position: 1 }),
  conversation('random', { position: 0 }),
  conversation('design', { category_id: 'product' }),
  conversation('left', { member_ids: ['you'] }),
  conversation('old', { archived: true }),
  conversation('dm-ada', { kind: 'dm', name: '', member_ids: ['me', 'ada'], last_message_at: '2026-10-20T00:00:00Z' }),
  conversation('dm-me', { kind: 'dm', name: '', member_ids: ['me'], self_dm: true }),
  conversation('dm-bob', { kind: 'dm', name: '', member_ids: ['me', 'bob'], last_message_at: '2026-10-22T00:00:00Z' }),
  conversation('starred', { position: 5 }),
]

test('sections follow the web sidebar: favorites, categories, channels, then DMs with your own first', () => {
  const sections = chatSections(CONVERSATIONS, [{ id: 'product', name: 'Product', position: 0 }, { id: 'empty', name: 'Empty', position: 1 }], [state('starred', { favorite: true })], 'me')
  expect(sections.map((section) => [section.title, section.conversations.map((item) => item.id)])).toEqual([
    ['Favorites', ['starred']],
    ['Product', ['design']],
    ['Channels', ['random', 'general']],
    ['Direct messages', ['dm-me', 'dm-bob', 'dm-ada']],
  ])
})

test('unread count: all unread where notify is all, mentions elsewhere, nothing from left conversations', () => {
  const states = [
    state('general', { notify: 'all', unread_count: 4, mention_count: 1 }),
    state('random', { notify: 'mentions', unread_count: 9, mention_count: 2 }),
    state('design', { notify: 'muted', unread_count: 7, mention_count: 1 }),
    state('left', { notify: 'all', unread_count: 5 }),
  ]
  expect(chatBadge(CONVERSATIONS, states, 'me')).toEqual({ count: 7, unread: true })
})

test('unread without a count: a channel with only messages that do not mention you', () => {
  expect(chatBadge(CONVERSATIONS, [state('random', { notify: 'mentions', unread_count: 3 })], 'me')).toEqual({ count: 0, unread: true })
  // muted and left conversations do not count
  const quiet = [state('design', { notify: 'muted', unread_count: 7 }), state('left', { notify: 'all', unread_count: 5 })]
  expect(chatBadge(CONVERSATIONS, quiet, 'me')).toEqual({ count: 0, unread: false })
})

test('DM titles and mention tokens use names', () => {
  const people = [
    { id: 'me', name: 'Orbit Developer' },
    { id: 'ada', name: 'Ada Lovelace' },
  ]
  expect(conversationTitle(CONVERSATIONS[5]!, people, 'me')).toBe('Ada Lovelace')
  expect(conversationTitle(CONVERSATIONS[6]!, people, 'me')).toBe('Orbit Developer (you)')
  expect(decodeMentions('hi <@ada> see <#general>, <!here> <@zed>', people, [{ id: 'general', name: 'general' }])).toBe('hi @Ada Lovelace see #general, @here @unknown')
})

test('rows: day separators, and a group continues for the same author within five minutes', () => {
  const rows = buildMessageRows([
    message('m1', { created_at: '2026-10-22T10:00:00Z' }),
    message('m2', { created_at: '2026-10-23T10:00:00Z' }),
    message('m3', { created_at: '2026-10-23T10:04:00Z' }),
    message('m4', { created_at: '2026-10-23T10:05:00Z', author_id: 'bob' }),
    message('m5', { created_at: '2026-10-23T10:06:00Z', author_id: 'bob', reply_to_id: 'm4' }),
  ])
  expect(rows.map((row) => (row.type === 'day' ? 'day' : `${row.message.id}:${row.groupStart ? 'start' : 'cont'}`))).toEqual([
    'day',
    'm1:start',
    'day',
    'm2:start',
    'm3:cont',
    'm4:start',
    'm5:start',
  ])
})

const pages = (...items: string[][]): MessagePages => ({
  pages: items.map((ids, index) => ({ items: ids.map((id) => message(id, {})), before: index === 0 ? null : 'x', after: index === items.length - 1 ? null : 'y' })),
  pageParams: items.map(() => undefined),
})
const ids = (data: MessagePages) => data.pages.map((page) => page.items.map((item) => `${item.id}:${item.body}`))

test('a new message joins the newest page; an edit replaces its copy; a thread reply stays out', () => {
  let data = pages(['m1'], ['m2'])
  data = upsertMessage(data, message('m3', {}))
  data = upsertMessage(data, message('m1', { body: 'edited' }))
  data = upsertMessage(data, message('r1', { thread_root_id: 'm1' }))
  expect(ids(data)).toEqual([['m1:edited'], ['m2:m2', 'm3:m3']])
  expect(ids(removeMessage(data, 'm2'))).toEqual([['m1:edited'], ['m3:m3']])
})

test('a new message is not appended when the newest page is not loaded', () => {
  const data: MessagePages = { pages: [{ items: [message('m1', {})], before: null, after: 'm2' }], pageParams: [undefined] }
  expect(ids(upsertMessage(data, message('m9', {})))).toEqual([['m1:m1']])
})

// ---- actions and permissions

const member = (user_id: string, display_name: string, fields: Partial<MemberRecord> = {}): MemberRecord =>
  ({ id: `m-${user_id}`, user_id, display_name, role: 'Member', email: `${user_id}@x`, suspended_at: null, ...fields }) as MemberRecord

test('row and header sheets follow the web menu: a member of a private channel they made', () => {
  const channel = conversation('secret', { kind: 'private', member_ids: ['me', 'ada'] })
  const permissions = conversationPermissions(channel, 'me', false)
  expect(permissions).toEqual({ canManage: true, canArchive: true, canLeave: true, canAddPeople: true, canRemovePeople: true })
  expect(conversationActions('row', state('secret', { unread_count: 2 }), permissions)).toEqual(['mark-read', 'favorite', 'notify', 'edit', 'leave', 'archive'])
  expect(conversationActions('header', state('secret', {}), permissions)).toEqual(['threads', 'pins', 'files', 'members', 'favorite', 'notify', 'edit', 'leave', 'archive'])
})

test('#general: an admin edits it but cannot archive it or add people; a plain member only stars and mutes', () => {
  const general = conversation('general', { is_default: true, created_by: 'ada' })
  expect(conversationPermissions(general, 'me', true)).toEqual({ canManage: true, canArchive: false, canLeave: false, canAddPeople: false, canRemovePeople: false })
  expect(conversationActions('row', state('general', {}), conversationPermissions(general, 'me', false))).toEqual(['favorite', 'notify'])
  expect(conversationPermissions(conversation('dm', { kind: 'dm', member_ids: ['me', 'ada'] }), 'me', true).canAddPeople).toBe(false)
})

test('message sheet: own text gets edit and delete, another author only for a moderator, a deleted one keeps the thread', () => {
  const own = message('m1', { author_id: 'me', pinned: false, sticker_id: null, body: 'hi' })
  expect(messageActions(own, 'me', false, { inThread: false, canWrite: true })).toEqual(['reply', 'thread', 'copy', 'pin', 'unread', 'edit', 'delete'])
  const theirs = message('m2', { author_id: 'ada', pinned: true, sticker_id: null, body: 'hi' })
  expect(messageActions(theirs, 'me', false, { inThread: true, canWrite: true })).toEqual(['reply', 'copy', 'unpin', 'unread'])
  expect(messageActions(theirs, 'me', true, { inThread: true, canWrite: false })).toEqual(['copy', 'unpin', 'unread', 'delete'])
  expect(messageActions(message('m3', { deleted: true }), 'me', true, { inThread: false, canWrite: true })).toEqual(['thread'])
  expect(messageActions(message('m4', { kind: 'join' }), 'me', true, { inThread: false, canWrite: true })).toEqual([])
})

test('channel names are typed lowercase with hyphens, and sent without a trailing one', () => {
  expect(channelNameInput('#Design Team ')).toBe('design-team-')
  expect(channelNameFinal('#Design  Team -')).toBe('design-team')
})

test('a new message offers yourself first, then the others by name, without suspended people', () => {
  const members = [member('zoe', 'Zoe'), member('me', 'Orbit Dev'), member('ada', 'Ada'), member('old', 'Old', { suspended_at: '2026-01-01T00:00:00Z' })]
  expect(messageablePeople(members, 'me')).toEqual([
    { id: 'me', name: 'Orbit Dev (you)' },
    { id: 'ada', name: 'Ada' },
    { id: 'zoe', name: 'Zoe' },
  ])
  expect(pickablePeople(members, 'me', ['zoe'])).toEqual([{ id: 'ada', name: 'Ada' }])
})

// ---- reactions

test('a tap adds a reaction, joins one, takes yours back, and drops it when nobody holds it', () => {
  let item = message('m1', { reactions: [{ emoji: '👍', user_ids: ['ada'] }] })
  item = toggleReaction(item, '👍', 'me')
  item = toggleReaction(item, ':parrot:', 'me')
  expect(item.reactions).toEqual([
    { emoji: '👍', user_ids: ['ada', 'me'] },
    { emoji: ':parrot:', user_ids: ['me'] },
  ])
  item = toggleReaction(item, ':parrot:', 'me')
  item = toggleReaction(item, '👍', 'ada')
  expect(item.reactions).toEqual([{ emoji: '👍', user_ids: ['me'] }])
  expect(hasReaction(item, '👍', 'me')).toBe(true)
  expect(hasReaction(item, '👍', 'ada')).toBe(false)
})

test('quick reactions: your most used first, then the defaults', () => {
  const messages = [
    message('m1', { reactions: [{ emoji: '🎉', user_ids: ['me'] }, { emoji: '👍', user_ids: ['me'] }] }),
    message('m2', { reactions: [{ emoji: '🎉', user_ids: ['me', 'ada'] }, { emoji: '🔥', user_ids: ['ada'] }] }),
  ]
  expect(quickReactions(messages, 'me')).toEqual(['🎉', '👍', '👀'])
  expect(customEmojiName(':party_parrot:')).toBe('party_parrot')
  expect(customEmojiName('🎉')).toBe(null)
})

// ---- unreads and threads

test('unreads: unread, unmuted conversations by activity, and the unread messages after the cursor', () => {
  const states = [
    state('general', { unread_count: 2 }),
    state('dm-bob', { unread_count: 1, notify: 'mentions' }),
    state('random', { unread_count: 3, notify: 'muted' }),
    state('design', { unread_count: 0 }),
  ]
  expect(unreadConversations(CONVERSATIONS, states, 'me').map((group) => group.conversation.id)).toEqual(['dm-bob', 'general'])
  const messages = [message('01', {}), message('02', { author_id: 'me' }), message('03', {}), message('04', { kind: 'join' }), message('05', {})]
  expect(unreadMessages(messages, state('general', { last_read_message_id: '01' }), 'me').map((item) => item.id)).toEqual(['03', '05'])
})

const followed = (id: string, unread: number, lastReply: string, following = true): FollowedThreadRecord => ({
  conversation_id: 'c1',
  root: message(id, { last_reply_at: lastReply }),
  last_reply: null,
  state: { conversation_id: 'c1', root_id: id, following, unread_replies: unread, mention_count: 0, last_read_reply_id: null },
})

test('threads: unread first, then newest reply; unfollowed ones leave', () => {
  const threads = [followed('a', 0, '2026-10-23T09:00:00Z'), followed('b', 2, '2026-10-20T09:00:00Z'), followed('c', 0, '2026-10-23T10:00:00Z'), followed('d', 5, '2026-10-23T11:00:00Z', false)]
  expect(sortFollowedThreads(threads).map((thread) => thread.root.id)).toEqual(['b', 'c', 'a'])
  expect(unreadThreadCount(threads)).toBe(1)
})

test('a thread cache takes the root, new and edited replies, and ignores other threads', () => {
  const page = (items: MessageRecord[]): ThreadPage => ({ root: message('root', {}), items, before: null, after: null, state: null })
  let data: ThreadPages = { pages: [page([message('r1', { thread_root_id: 'root' })])], pageParams: [undefined] }
  data = upsertThreadMessage(data, message('r2', { thread_root_id: 'root' }))
  data = upsertThreadMessage(data, message('r1', { thread_root_id: 'root', body: 'edited' }))
  data = upsertThreadMessage(data, message('x1', { thread_root_id: 'other' }))
  data = upsertThreadMessage(data, message('root', { reply_count: 2 }))
  expect(data.pages[0]!.items.map((item) => `${item.id}:${item.body}`)).toEqual(['r1:edited', 'r2:r2'])
  expect(data.pages[0]!.root.reply_count).toBe(2)
})

// ---- search

test('search hits are cut at their ranges, overlaps merged, and a long lead shortened from a word', () => {
  expect(splitByRanges('deploy the deploy bot', [[11, 17], [0, 6], [4, 9]])).toEqual([
    { text: 'deploy th', match: true },
    { text: 'e ', match: false },
    { text: 'deploy', match: true },
    { text: ' bot', match: false },
  ])
  const lead = leadToFirstMatch(splitByRanges('one two three four five six seven eight nine ten match', [[49, 54]]), 20)
  expect(lead).toEqual([
    { text: '…eight nine ten ', match: false },
    { text: 'match', match: true },
  ])
})

// ---- emoji

test('emoji groups come from emojibase in order, without components or newer emoji, and search by shortcode then label', () => {
  const data: EmojibaseData = {
    groups: [
      { key: 'people-body', message: 'people & body', order: 1 },
      { key: 'smileys-emotion', message: 'smileys & emotion', order: 0 },
      { key: 'component', message: 'components', order: 2 },
    ],
    emojis: [
      { emoji: '😂', hexcode: '1F602', label: 'face with tears of joy', group: 0, order: 2, version: 0.6 },
      { emoji: '😀', hexcode: '1F600', label: 'grinning face', group: 0, order: 1, version: 1 },
      { emoji: '👋', hexcode: '1F44B', label: 'waving hand', group: 1, order: 1, version: 0.6 },
      { emoji: '🏻', hexcode: '1F3FB', label: 'light skin tone', group: 2, order: 1, version: 1 },
      { emoji: '🫪', hexcode: '1FAEA', label: 'future face', group: 0, order: 3, version: 18 },
    ],
    shortcodes: { '1F602': 'joy', '1F600': ['grinning', 'smile_big'], '1F44B': 'wave' },
  }
  const groups = emojiGroups(data)
  expect(groups.map((group) => [group.title, group.emojis.map((entry) => entry.emoji).join('')])).toEqual([
    ['Smileys & emotion', '😀😂'],
    ['People & body', '👋'],
  ])
  expect(searchEmoji(groups, ':joy').map((entry) => entry.emoji)).toEqual(['😂'])
  expect(searchEmoji(groups, 'face').map((entry) => entry.emoji)).toEqual(['😂', '😀'])
  expect(searchEmoji(groups, 'ing').map((entry) => entry.emoji)).toEqual(['😀', '👋'])
})

test('file sizes read like the web', () => {
  expect([fileSize(512), fileSize(12_800), fileSize(3_565_158)]).toEqual(['512 B', '13 KB', '3.4 MB'])
})
