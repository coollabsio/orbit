import { expect, test } from 'bun:test'
import { createApiClient } from '@/api/client'
import { createHttpChatClient } from './httpClient'
import type { LiveSocketOptions } from './liveSocket'
import type { ChatEvent } from './types'

const state = { conversation_id: 'c1', last_read_message_id: 'm9', unread_count: 0, mention_count: 0, notify: 'all', favorite: false }

function setup(respond: (path: string) => Response) {
  const requests: string[] = []
  const sent: object[] = []
  /** The number of the newest event that the fake socket has delivered. */
  const seen = { seq: 0 }
  /** The live socket's callbacks, so a test can play the server. */
  let live!: LiveSocketOptions
  const client = createHttpChatClient({
    workspaceId: 'w1',
    currentUserId: 'u1',
    readIntervalMs: 40,
    openSocket: (options) => {
      live = options
      return { send: (frame) => sent.push(frame), seen: (seq) => seq <= seen.seq, reconnected: () => Promise.resolve(), close() {} }
    },
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
  return { client, requests, events, sent, live, seen }
}

test('a write gives its events to the subscribers and its result to the caller', async () => {
  const { client, requests, events } = setup(() => Response.json({ result: state, events: [{ type: 'state.changed', state }], seq: 5 }))
  const result = await client.setFavorite('c1', false)
  expect(requests).toEqual(['PATCH /api/v1/workspaces/w1/chat/conversations/c1/state'])
  expect(result.lastReadMessageId).toBe('m9')
  expect(events).toEqual([{ type: 'state.changed', state: result }])
})

test('a response that comes after the socket delivered its events does not go over newer events', async () => {
  const { client, events, seen } = setup(() => Response.json({ result: state, events: [{ type: 'state.changed', state }], seq: 5 }))
  seen.seq = 6
  const result = await client.setFavorite('c1', false)
  expect(result.lastReadMessageId).toBe('m9')
  expect(events).toEqual([])
})

test('"mark as read" calls inside one interval share one more request', async () => {
  const { client, requests } = setup(() => Response.json({ result: state, events: [], seq: 1 }))
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

test('the live socket gives presence, typing and chat events to the subscribers', async () => {
  const { client, events, sent, live } = setup(() => Response.json({}))
  const presence = client.getPresence()
  live.onHello(['u1', 'u2'])
  expect(await presence).toEqual(['u1', 'u2'])
  live.onEvent('presence', { type: 'presence', user_id: 'u3', online: true })
  live.onEvent('chat', { type: 'typing', conversation_id: 'c1', thread_root_id: null, user_id: 'u2' })
  live.onEvent('chat', { type: 'state.changed', state })
  live.onStatus('reconnecting')
  // After a reconnect the new list says who left and who came meanwhile.
  live.onHello(['u1', 'u3', 'u4'])
  expect(events).toEqual([
    { type: 'presence', userId: 'u3', online: true },
    { type: 'typing', conversationId: 'c1', threadRootId: null, userId: 'u2' },
    { type: 'state.changed', state: { conversationId: 'c1', lastReadMessageId: 'm9', unreadCount: 0, mentionCount: 0, notify: 'all', favorite: false } },
    { type: 'connection', status: 'reconnecting' },
    { type: 'presence', userId: 'u2', online: false },
    { type: 'presence', userId: 'u4', online: true },
  ])
  client.sendTyping('c1')
  expect(sent).toEqual([{ type: 'typing', conversation_id: 'c1', thread_root_id: null }])
})

test('a send that cannot reach the server goes again when the connection is back', async () => {
  let attempts = 0
  const message = {
    id: 'm1', conversation_id: 'c1', thread_root_id: null, kind: 'message', author_id: 'u1', body: 'Hi',
    mentions: { user_ids: [], channel: false, here: false }, created_at: '2026-10-02T08:00:00.000Z', edited_at: null,
    deleted: false, attachments: [], reactions: [], pinned: false, also_in_channel: false, nonce: 'n1', reply_count: 0, last_reply_at: null,
    reply_user_ids: [], last_reply: null,
  }
  const { client, requests } = setup(() => {
    attempts += 1
    if (attempts === 1) throw new TypeError('Failed to fetch')
    return Response.json({ result: message, events: [], seq: 1 })
  })
  const sentMessage = await client.sendMessage({ conversationId: 'c1', body: 'Hi', nonce: 'n1' })
  expect(sentMessage.id).toBe('m1')
  expect(requests.length).toBe(2)
})

test('a send that the server refused does not go again', async () => {
  const problem = { type: 'about:blank', title: 'Too many requests', status: 429, code: 'rate_limited', detail: 'Slow down.', instance: '/x', request_id: 'r' }
  const { client, requests } = setup(() => Response.json(problem, { status: 429, headers: { 'content-type': 'application/problem+json' } }))
  expect(client.sendMessage({ conversationId: 'c1', body: 'Hi', nonce: 'n1' })).rejects.toMatchObject({ name: 'ChatError', unreached: false })
  await new Promise((resolve) => setTimeout(resolve, 10))
  expect(requests.length).toBe(1)
})
