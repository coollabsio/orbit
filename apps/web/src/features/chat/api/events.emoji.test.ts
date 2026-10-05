import { expect, test } from 'bun:test'
import { QueryClient } from '@tanstack/react-query'
import { queryKeys } from '@/api/queryKeys'
import { applyChatEvent } from './events'
import { chatKeys } from './keys'

const W = 'w1'

function setup() {
  const client = new QueryClient()
  client.setQueryData(queryKeys.customEmoji(W), [])
  client.setQueryData(queryKeys.customEmoji('w2'), [])
  client.setQueryData(chatKeys.conversations(W), [])
  const stale = (key: readonly unknown[]) => client.getQueryState(key)!.isInvalidated
  return { client, stale }
}

test('emoji.changed fetches the custom emoji of this workspace again, and nothing else', () => {
  const { client, stale } = setup()
  applyChatEvent(client, W, { type: 'emoji.changed' })
  expect(stale(queryKeys.customEmoji(W))).toBe(true)
  expect(stale(queryKeys.customEmoji('w2'))).toBe(false)
  expect(stale(chatKeys.conversations(W))).toBe(false)
})

test('a resync fetches the custom emoji again too: their event may be among the missed ones', () => {
  const { client, stale } = setup()
  applyChatEvent(client, W, { type: 'resync' })
  expect(stale(queryKeys.customEmoji(W))).toBe(true)
  expect(stale(chatKeys.conversations(W))).toBe(true)
})
