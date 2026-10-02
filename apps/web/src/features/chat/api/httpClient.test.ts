import { expect, test } from 'bun:test'
import { createApiClient } from '@/api/client'
import { createHttpChatClient } from './httpClient'
import type { ChatEvent } from './types'

const state = { conversation_id: 'c1', last_read_message_id: 'm9', unread_count: 0, mention_count: 0, notify: 'all', favorite: false }

function setup(respond: (path: string) => Response) {
  const requests: string[] = []
  const client = createHttpChatClient({
    workspaceId: 'w1',
    currentUserId: 'u1',
    readIntervalMs: 40,
    client: createApiClient({
      fetch: async (request) => {
        const path = `${request.method} ${new URL(request.url).pathname}`
        requests.push(path)
        return respond(path)
      },
    }),
  })
  const events: ChatEvent[] = []
  client.subscribe((event) => events.push(event))
  return { client, requests, events }
}

test('a write gives its events to the subscribers and its result to the caller', async () => {
  const { client, requests, events } = setup(() => Response.json({ result: state, events: [{ type: 'state.changed', state }] }))
  const result = await client.setFavorite('c1', false)
  expect(requests).toEqual(['PATCH /api/v1/workspaces/w1/chat/conversations/c1/state'])
  expect(result.lastReadMessageId).toBe('m9')
  expect(events).toEqual([{ type: 'state.changed', state: result }])
})

test('"mark as read" calls inside one interval share one more request', async () => {
  const { client, requests } = setup(() => Response.json({ result: state, events: [] }))
  await client.markRead('c1')
  const second = client.markRead('c1')
  const third = client.markRead('c1')
  expect(third).toBe(second)
  // Another conversation has its own interval.
  await client.markRead('c2')
  expect(requests.length).toBe(2)
  await second
  expect(requests.filter((path) => path.endsWith('/c1/read')).length).toBe(2)
})

test('a refused request rejects with a chat error', async () => {
  const problem = { type: 'about:blank', title: 'Action forbidden', status: 403, code: 'chat_forbidden', detail: 'Join the channel first.', instance: '/x', request_id: 'r' }
  const { client, events } = setup(() => Response.json(problem, { status: 403, headers: { 'content-type': 'application/problem+json' } }))
  expect(client.markUnread('m1')).rejects.toMatchObject({ name: 'ChatError', code: 'forbidden', message: 'Join the channel first.' })
  expect(events).toEqual([])
})
