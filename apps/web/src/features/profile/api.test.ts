import { expect, test } from 'bun:test'
import { QueryClient } from '@tanstack/react-query'
import type { MemberProfile } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { saveMemberNote } from './api'

const key = queryKeys.memberProfile('w1', 'u2')

function cacheWith(note: string | null) {
  const client = new QueryClient()
  client.setQueryData<MemberProfile>(key, { user_id: 'u2', title: 'Engineer', note })
  return client
}

function recorder(result: () => Promise<void> = async () => {}) {
  const sent: string[] = []
  return { sent, send: (_workspaceId: string, _userId: string, body: string) => (sent.push(body), result()) }
}

const noteOf = (client: QueryClient) => client.getQueryData<MemberProfile>(key)?.note

test('an unchanged note sends nothing', async () => {
  const client = cacheWith('likes tea')
  const { sent, send } = recorder()
  expect(await saveMemberNote(client, 'w1', 'u2', 'likes tea ', send)).toBe(false)
  expect(sent).toEqual([])
})

test('a changed note is in the cache at once and is sent once; the same text again sends nothing', async () => {
  const client = cacheWith(null)
  const { sent, send } = recorder()
  const saving = saveMemberNote(client, 'w1', 'u2', ' likes coffee ', send)
  expect(noteOf(client)).toBe('likes coffee')
  await saving
  // the blur saved it; the close of the popover then has nothing to do
  await saveMemberNote(client, 'w1', 'u2', ' likes coffee ', send)
  expect(sent).toEqual(['likes coffee'])
  expect(client.getQueryData<MemberProfile>(key)?.title).toBe('Engineer')
})

test('an emptied note is removed: empty text goes out and the cache has no note', async () => {
  const client = cacheWith('likes tea')
  const { sent, send } = recorder()
  await saveMemberNote(client, 'w1', 'u2', '   ', send)
  expect(sent).toEqual([''])
  expect(noteOf(client)).toBeNull()
})

test('a refused save puts the previous note back', async () => {
  const client = cacheWith('likes tea')
  const { send } = recorder(async () => {
    throw new Error('offline')
  })
  await saveMemberNote(client, 'w1', 'u2', 'likes coffee', send)
  expect(noteOf(client)).toBe('likes tea')
})

test('a refused save leaves a newer note alone', async () => {
  const client = cacheWith('likes tea')
  let fail!: () => void
  const first = saveMemberNote(client, 'w1', 'u2', 'first', () => new Promise<void>((_resolve, reject) => (fail = () => reject(new Error('offline')))))
  await saveMemberNote(client, 'w1', 'u2', 'second', async () => {})
  fail()
  await first
  expect(noteOf(client)).toBe('second')
})

test('nothing is saved for a profile that is not loaded', async () => {
  const { sent, send } = recorder()
  expect(await saveMemberNote(new QueryClient(), 'w1', 'u2', 'text', send)).toBe(false)
  expect(sent).toEqual([])
})
