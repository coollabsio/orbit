import { expect, spyOn, test } from 'bun:test'
import { QueryClient } from '@tanstack/react-query'
import { emitMailEvent } from './events'
import { mailKeys } from './keys'

const W = 'w1'

function setup() {
  const client = new QueryClient()
  const keys = {
    inbox: mailKeys.threads(W, { folder: 'inbox' }),
    label: mailKeys.threads(W, { labelId: 'l1' }),
    search: mailKeys.search(W, 'from:ada'),
    counts: mailKeys.counts(W),
    thread: mailKeys.thread(W, 't1'),
    otherThread: mailKeys.thread(W, 't2'),
    me: mailKeys.me(W),
    domains: mailKeys.domains(W),
    viewUrl: mailKeys.viewUrl(W, 'm1', false),
    otherWorkspace: mailKeys.counts('w2'),
  }
  for (const key of Object.values(keys)) client.setQueryData(key, {})
  const invalidated = () =>
    Object.entries(keys)
      .filter(([, key]) => client.getQueryState(key)?.isInvalidated)
      .map(([name]) => name)
      .sort()
  return { client, invalidated }
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0))

test('thread.updated invalidates exactly the thread, the thread lists and the counts', async () => {
  const { client, invalidated } = setup()
  const invalidate = spyOn(client, 'invalidateQueries')
  emitMailEvent(client, W, { type: 'thread.updated', mailbox_id: 'mb1', thread_id: 't1' })
  await settle()
  expect(invalidated()).toEqual(['counts', 'inbox', 'label', 'search', 'thread'])
  expect(invalidate.mock.calls.map(([filters]) => filters?.queryKey)).toEqual([
    mailKeys.lists(W),
    mailKeys.counts(W),
    mailKeys.thread(W, 't1'),
  ])
})

test('events of another kind or shape change nothing', async () => {
  const { client, invalidated } = setup()
  emitMailEvent(client, W, { type: 'thread.deleted', mailbox_id: 'mb1', thread_id: 't1' })
  emitMailEvent(client, W, { type: 'thread.updated', mailbox_id: 'mb1' })
  emitMailEvent(client, W, null)
  await settle()
  expect(invalidated()).toEqual([])
})

test('events that arrive during a refresh wait for it and then go out together', async () => {
  const { client } = setup()
  const pending: Array<() => void> = []
  const invalidate = spyOn(client, 'invalidateQueries').mockImplementation(
    () => new Promise<void>((resolve) => void pending.push(resolve)),
  )
  const keys = () => invalidate.mock.calls.map(([filters]) => filters?.queryKey)
  emitMailEvent(client, W, { type: 'thread.updated', mailbox_id: 'mb1', thread_id: 't1' })
  await settle()
  expect(keys()).toHaveLength(3)
  emitMailEvent(client, W, { type: 'thread.updated', mailbox_id: 'mb1', thread_id: 't2' })
  emitMailEvent(client, W, { type: 'thread.updated', mailbox_id: 'mb2', thread_id: 't3' })
  await settle()
  expect(keys()).toHaveLength(3)
  for (const resolve of pending.splice(0)) resolve()
  await settle()
  expect(keys().slice(3)).toEqual([
    mailKeys.lists(W),
    mailKeys.counts(W),
    mailKeys.thread(W, 't2'),
    mailKeys.thread(W, 't3'),
  ])
})
