import { expect, test } from 'bun:test'
import type { Conversation } from '../api/types'
import { canEdit, canForward, FORWARD_LIMIT, forwardDestinations, forwardToEach, toggleDestination } from './forward'
import { testMessage } from './testMessage'

const conversation = (id: string, overrides: Partial<Conversation> = {}): Conversation => ({
  id, kind: 'public', name: id, topic: '', categoryId: null, position: 0, memberIds: ['u1'], isMember: true,
  isDefault: false, selfDm: false, archived: false, createdBy: 'u1', createdAt: 0, lastMessageAt: null, ...overrides,
})
const titleOf = (item: Conversation) => (item.kind === 'dm' ? 'Ada Lovelace' : `#${item.name}`)
const origin = { messageId: 'm0', conversationId: 'c2', authorId: 'u2', createdAt: 0 }

test('destinations are the joined channels and DMs that are not archived, latest activity first', () => {
  const all = [
    conversation('general', { lastMessageAt: 10 }),
    conversation('unjoined', { isMember: false }),
    conversation('old', { archived: true }),
    conversation('secret', { kind: 'private', lastMessageAt: 30 }),
    conversation('dm1', { kind: 'dm', name: '', lastMessageAt: 20 }),
  ]
  expect(forwardDestinations(all, titleOf).map((item) => item.id)).toEqual(['secret', 'dm1', 'general'])
})

test('the search matches the title without regard to case, with or without #', () => {
  const all = [conversation('general'), conversation('design'), conversation('dm1', { kind: 'dm', name: '' })]
  const ids = (search: string) => forwardDestinations(all, titleOf, search).map((item) => item.id)
  expect(ids(' #GEN ')).toEqual(['general'])
  expect(ids('love')).toEqual(['dm1'])
  expect(ids('nothing')).toEqual([])
})

test('a destination toggles, and one more than the limit is not added', () => {
  expect(toggleDestination(['a'], 'b')).toEqual(['a', 'b'])
  expect(toggleDestination(['a', 'b'], 'a')).toEqual(['b'])
  const full = Array.from({ length: FORWARD_LIMIT }, (_, index) => `c${index}`)
  expect(toggleDestination(full, 'more')).toEqual(full)
  expect(toggleDestination(full, 'c0')).toHaveLength(FORWARD_LIMIT - 1)
})

test('each destination gets the forward, then the comment, one after another', async () => {
  const calls: string[] = []
  const result = await forwardToEach(
    ['a', 'b'],
    async (id) => {
      await Promise.resolve()
      calls.push(`forward:${id}`)
    },
    (id) => calls.push(`comment:${id}`),
  )
  expect(calls).toEqual(['forward:a', 'comment:a', 'forward:b', 'comment:b'])
  expect(result).toEqual({ sentIds: ['a', 'b'] })
})

test('the first failure stops the run: no comment there, and the rest is not tried', async () => {
  const calls: string[] = []
  const refused = new Error('chat_forbidden')
  const result = await forwardToEach(
    ['a', 'b', 'c'],
    async (id) => {
      calls.push(`forward:${id}`)
      if (id === 'b') throw refused
    },
    (id) => calls.push(`comment:${id}`),
  )
  expect(calls).toEqual(['forward:a', 'comment:a', 'forward:b'])
  expect(result).toEqual({ sentIds: ['a'], error: refused })
})

test('a confirmed message that is not deleted can be forwarded, a forward too', () => {
  expect(canForward(testMessage({ id: 'm1' }))).toBe(true)
  expect(canForward(testMessage({ id: 'm1', threadRootId: 'm0' }))).toBe(true)
  expect(canForward(testMessage({ id: 'm1', forwarded: origin }))).toBe(true)
  expect(canForward(testMessage({ id: 'm1', deleted: true }))).toBe(false)
  expect(canForward(testMessage({ id: 'm1', kind: 'join' }))).toBe(false)
  expect(canForward(testMessage({ id: 'm1', sendState: 'failed' }))).toBe(false)
})

test('only the own message can be edited, and never a forward', () => {
  expect(canEdit(testMessage({ id: 'm1' }), 'u1')).toBe(true)
  expect(canEdit(testMessage({ id: 'm1' }), 'u2')).toBe(false)
  expect(canEdit(testMessage({ id: 'm1', forwarded: origin }), 'u1')).toBe(false)
  expect(canEdit(testMessage({ id: 'm1', sendState: 'sending' }), 'u1')).toBe(false)
  // A row cached before forwards existed has no such field.
  expect(canEdit({ ...testMessage({ id: 'm1' }), forwarded: undefined as never }, 'u1')).toBe(true)
})

test('a message that is only a sticker cannot be edited; one with text can, and both can be forwarded', () => {
  const sticker = { id: 's1', name: 'Party Parrot', url: '/stickers/s1' }
  const only = testMessage({ id: 'm1', body: '', stickerId: 's1', sticker })
  expect(canEdit(only, 'u1')).toBe(false)
  expect(canForward(only)).toBe(true)
  // the sticker was deleted: still no text to edit
  expect(canEdit({ ...only, sticker: null }, 'u1')).toBe(false)
  expect(canEdit(testMessage({ id: 'm2', body: 'look', stickerId: 's1', sticker }), 'u1')).toBe(true)
  // a row cached before stickers existed has no such fields
  expect(canEdit({ ...testMessage({ id: 'm3' }), stickerId: undefined as unknown as null }, 'u1')).toBe(true)
})
