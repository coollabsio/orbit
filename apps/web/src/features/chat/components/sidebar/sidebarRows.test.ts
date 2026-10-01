import { expect, test } from 'bun:test'
import type { Conversation } from '@/features/chat/api/types'
import type { SidebarSection } from '@/features/chat/lib/sidebar'
import { adjacentConversation, DM_LIMIT, rowLabel, sidebarOrder, visibleRows } from './sidebarRows'

const conversation = (id: string, kind: Conversation['kind'] = 'public'): Conversation => ({
  id,
  kind,
  name: id,
  topic: '',
  categoryId: null,
  position: 0,
  memberIds: [],
  isMember: true,
  isDefault: false,
  archived: false,
  createdBy: 'u1',
  createdAt: 0,
  lastMessageAt: null,
})

const section = (key: string, kind: SidebarSection['kind'], ids: string[]): SidebarSection => ({
  key,
  kind,
  title: key,
  category: null,
  conversations: ids.map((id) => conversation(id, kind === 'dms' ? 'dm' : 'public')),
})

const NO_BADGE = { bold: false, count: 0, marker: false, inUnreads: false }

test('the order follows the sections from top to bottom', () => {
  expect(sidebarOrder([section('favorites', 'favorites', ['c3']), section('channels', 'channels', ['c1', 'c2']), section('dms', 'dms', ['d1'])])).toEqual(['c3', 'c1', 'c2', 'd1'])
})

test('previous and next move one row and wrap around', () => {
  const ids = ['a', 'b', 'c']
  expect(adjacentConversation(ids, 'a', 'next')).toBe('b')
  expect(adjacentConversation(ids, 'c', 'next')).toBe('a')
  expect(adjacentConversation(ids, 'a', 'previous')).toBe('c')
  expect(adjacentConversation(ids, 'b', 'previous')).toBe('a')
})

test('with nothing open, next starts at the top and previous at the bottom', () => {
  const ids = ['a', 'b', 'c']
  expect(adjacentConversation(ids, null, 'next')).toBe('a')
  expect(adjacentConversation(ids, null, 'previous')).toBe('c')
  // an open conversation that is not in the sidebar (a public channel the user has not joined)
  expect(adjacentConversation(ids, 'x', 'next')).toBe('a')
})

test('previous and next unread skip read rows and never return the open one', () => {
  const ids = ['a', 'b', 'c', 'd']
  const unread = (id: string) => id === 'a' || id === 'd'
  expect(adjacentConversation(ids, 'b', 'next', unread)).toBe('d')
  expect(adjacentConversation(ids, 'd', 'next', unread)).toBe('a')
  expect(adjacentConversation(ids, 'b', 'previous', unread)).toBe('a')
  expect(adjacentConversation(ids, 'a', 'next', (id) => id === 'a')).toBeNull()
  expect(adjacentConversation(ids, 'b', 'next', () => false)).toBeNull()
  expect(adjacentConversation(['a'], 'a', 'next')).toBeNull()
  expect(adjacentConversation([], null, 'next')).toBeNull()
})

test('a collapsed section keeps its unread rows and the open conversation', () => {
  const channels = section('channels', 'channels', ['a', 'b', 'c'])
  const shown = visibleRows(channels, { collapsed: true, showAllDms: false, activeId: 'c', isUnread: (id) => id === 'a' })
  expect(shown.rows.map((row) => row.id)).toEqual(['a', 'c'])
  expect(shown.hidden).toBe(0)
})

test('direct messages show ten, then the rest behind "Show more"', () => {
  const ids = Array.from({ length: 14 }, (_, index) => `d${index}`)
  const dms = section('dms', 'dms', ids)
  const base = { collapsed: false, activeId: null, isUnread: () => false }
  const first = visibleRows(dms, { ...base, showAllDms: false })
  expect(first.rows).toHaveLength(DM_LIMIT)
  expect(first.hidden).toBe(4)
  expect(visibleRows(dms, { ...base, showAllDms: true })).toEqual({ rows: dms.conversations, hidden: 0 })
  // an unread or open DM below the limit still shows
  const kept = visibleRows(dms, { ...base, showAllDms: false, activeId: 'd12', isUnread: (id) => id === 'd13' })
  expect(kept.rows.map((row) => row.id).slice(DM_LIMIT)).toEqual(['d12', 'd13'])
  expect(kept.hidden).toBe(2)
  // the limit is for direct messages only
  expect(visibleRows(section('channels', 'channels', ids), { ...base, showAllDms: false }).rows).toHaveLength(14)
})

test('the row name includes the unread state and count', () => {
  expect(rowLabel('design', NO_BADGE, 'mentions')).toBe('design')
  expect(rowLabel('design', { bold: true, count: 3, marker: false, inUnreads: true }, 'mentions')).toBe('design, 3 unread mentions')
  expect(rowLabel('design', { bold: true, count: 1, marker: false, inUnreads: true }, 'mentions')).toBe('design, 1 unread mention')
  expect(rowLabel('Ada Lovelace', { bold: true, count: 2, marker: false, inUnreads: true }, 'all')).toBe('Ada Lovelace, 2 unread messages')
  expect(rowLabel('random', { bold: true, count: 0, marker: true, inUnreads: true }, 'mentions')).toBe('random, unread')
  expect(rowLabel('noise', { ...NO_BADGE, count: 1 }, 'muted')).toBe('noise, 1 unread mention, muted')
  expect(rowLabel('design', NO_BADGE, 'mentions', true)).toBe('design, draft')
})
