import { expect, test } from 'bun:test'
import type { Conversation } from '@/features/chat/api/types'
import { parseChatLocation } from '@/features/chat/chatRoutes'
import { COLUMN_MIN, homeConversation, orbitLinkTarget, PANE_WIDTH, paneLayout, rightPaneOf, taskTitleFromMessage } from './chatPageLib'

test('the pane fits when the column keeps its minimum next to the narrowest pane', () => {
  expect(paneLayout(COLUMN_MIN + PANE_WIDTH.min, 400)).toEqual({ fits: true, width: PANE_WIDTH.min })
  expect(paneLayout(COLUMN_MIN + PANE_WIDTH.min - 1, 400).fits).toBe(false)
  expect(paneLayout(1400, 400)).toEqual({ fits: true, width: 400 })
})

test('the pane width is clamped to its range and to what the column leaves', () => {
  expect(paneLayout(2000, 900).width).toBe(PANE_WIDTH.max)
  expect(paneLayout(2000, 100).width).toBe(PANE_WIDTH.min)
  // 900 wide: the column keeps 400, so the pane gets 500 at most
  expect(paneLayout(900, 560)).toEqual({ fits: true, width: 500 })
})

test('before the first measurement the pane is treated as fitting', () => {
  expect(paneLayout(null, 400)).toEqual({ fits: true, width: 400 })
})

test('only same-origin task and page URLs are link card targets', () => {
  const origin = 'https://orbit.test'
  expect(orbitLinkTarget('https://orbit.test/tasks/t1', origin)).toEqual({ kind: 'task', id: 't1' })
  expect(orbitLinkTarget('/docs/p1?block=b', origin)).toEqual({ kind: 'page', id: 'p1' })
  expect(orbitLinkTarget('https://other.test/tasks/t1', origin)).toBeNull()
  expect(orbitLinkTarget('https://orbit.test/tasks', origin)).toBeNull()
  expect(orbitLinkTarget('https://orbit.test/tasks/projects/p1/settings', origin)).toBeNull()
  expect(orbitLinkTarget('https://orbit.test/docs/trash', origin)).toBeNull()
  expect(orbitLinkTarget('https://orbit.test/chat/c1', origin)).toBeNull()
  expect(orbitLinkTarget('not a url', origin)).toBeNull()
  // task URLs name a task by identifier too, also inside a saved view
  expect(orbitLinkTarget('https://orbit.test/tasks/ENG-12', origin)).toEqual({ kind: 'task', id: 'ENG-12' })
  expect(orbitLinkTarget('/views/v1/ENG-12?x=1', origin)).toEqual({ kind: 'task', id: 'ENG-12' })
  expect(orbitLinkTarget('/views/v1', origin)).toBeNull()
  expect(orbitLinkTarget('/views/v1/ENG-12/more', origin)).toBeNull()
})

test('the task title is the first line with text, cut near 80 characters', () => {
  expect(taskTitleFromMessage('\n  Fix the login page  \nmore')).toBe('Fix the login page')
  expect(taskTitleFromMessage('')).toBe('')
  const long = 'word '.repeat(30).trim()
  const title = taskTitleFromMessage(long)
  expect(title.endsWith('…')).toBe(true)
  expect(title.length).toBeLessThanOrEqual(81)
  expect(title).not.toContain('wor…')
  expect(taskTitleFromMessage('x'.repeat(100))).toBe(`${'x'.repeat(80)}…`)
})

test('a thread takes the right pane before a search, and a search before a named pane', () => {
  const pane = (search: string) => rightPaneOf(parseChatLocation('/chat/c1', search))
  expect(pane('?thread=m1&q=hello&pane=members&m=r2')).toEqual({ kind: 'thread', rootId: 'm1', focusMessageId: 'r2' })
  expect(pane('?q=hello&pane=members')).toEqual({ kind: 'search', query: 'hello' })
  expect(pane('?pane=members')).toEqual({ kind: 'members' })
  expect(pane('?pane=nothing')).toBeNull()
  expect(pane('')).toBeNull()
  expect(rightPaneOf(parseChatLocation('/chat/c1/thread/m1', '?pane=members'))).toBeNull()
  expect(rightPaneOf(parseChatLocation('/chat/unreads', '?pane=members'))).toBeNull()
})

const conversation = (id: string, patch: Partial<Conversation> = {}): Conversation => ({
  id,
  kind: 'public',
  name: id,
  topic: '',
  categoryId: null,
  position: 0,
  memberIds: [],
  isMember: true,
  isDefault: false,
  selfDm: false,
  archived: false,
  createdBy: 'u1',
  createdAt: 0,
  lastMessageAt: null,
  ...patch,
})

test('/chat opens the last conversation, else #general, else the first joined one', () => {
  const list = [conversation('browse', { isMember: false }), conversation('design'), conversation('general', { isDefault: true })]
  expect(homeConversation(list, 'design')?.id).toBe('design')
  // a public channel the user only reads can be the last opened one
  expect(homeConversation(list, 'browse')?.id).toBe('browse')
  expect(homeConversation(list, 'gone')?.id).toBe('general')
  expect(homeConversation(list, null)?.id).toBe('general')
  expect(homeConversation([conversation('browse', { isMember: false }), conversation('design')], null)?.id).toBe('design')
  expect(homeConversation([conversation('old', { archived: true })], 'old')).toBeNull()
  expect(homeConversation([], null)).toBeNull()
})
