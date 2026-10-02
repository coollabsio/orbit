import { expect, test } from 'bun:test'
import { openLiveSocket, type SocketLike } from './liveSocket'

class FakeSocket implements SocketLike {
  onopen: (() => void) | null = null
  onmessage: ((event: { data: unknown }) => void) | null = null
  onclose: ((event: { code: number }) => void) | null = null
  sent: string[] = []
  readonly url: string
  constructor(url: string) {
    this.url = url
  }
  send(data: string) {
    this.sent.push(data)
  }
  close() {
    this.onclose?.({ code: 1000 })
  }
  frame(value: object) {
    this.onmessage?.({ data: JSON.stringify(value) })
  }
}

function setup() {
  const sockets: FakeSocket[] = []
  const log: string[] = []
  const live = openLiveSocket({
    url: 'ws://orbit/live',
    retryMs: 1,
    createSocket: (url) => {
      const socket = new FakeSocket(url)
      sockets.push(socket)
      return socket
    },
    onEvent: (topic, event) => log.push(`${topic}:${(event as { type: string }).type}`),
    onHello: (online) => log.push(`hello:${online.join(',')}`),
    onResync: () => log.push('resync'),
    onStatus: (status) => log.push(status),
  })
  const reconnected = async () => {
    const count = sockets.length
    while (sockets.length === count) await new Promise((resolve) => setTimeout(resolve, 1))
    return sockets.at(-1)!
  }
  return { live, sockets, log, reconnected }
}

test('a reconnect asks for the events after the last one it saw', async () => {
  const { live, sockets, log, reconnected } = setup()
  const first = sockets[0]
  expect(first.url).toBe('ws://orbit/live')
  first.frame({ type: 'hello', epoch: 'e1', seq: 4, online: ['u1'] })
  first.frame({ type: 'resync' })
  first.frame({ seq: 5, topic: 'chat', event: { type: 'message.created' } })
  first.frame({ topic: 'chat', event: { type: 'typing' } })
  first.frame({ type: 'ping' })
  expect(log).toEqual(['connected', 'hello:u1', 'resync', 'chat:message.created', 'chat:typing'])

  first.onclose?.({ code: 1006 })
  const second = await reconnected()
  // The signal had no number: the cursor stays at the last numbered event.
  expect(second.url).toBe('ws://orbit/live?epoch=e1&after=5')
  expect(log.at(-1)).toBe('reconnecting')
  // A frame cannot go out while the socket reconnects.
  live.send({ type: 'typing' })
  expect(second.sent).toEqual([])
  second.frame({ type: 'hello', epoch: 'e1', seq: 9, online: [] })
  live.send({ type: 'typing' })
  expect(second.sent).toEqual(['{"type":"typing"}'])
  live.close()
})

test('after a resync the cursor is the number the server said hello with', async () => {
  const { live, sockets, reconnected } = setup()
  sockets[0].frame({ type: 'hello', epoch: 'e1', seq: 7, online: [] })
  sockets[0].frame({ type: 'resync' })
  sockets[0].onclose?.({ code: 1006 })
  expect((await reconnected()).url).toBe('ws://orbit/live?epoch=e1&after=7')
  live.close()
})

test('an ended session does not reconnect', async () => {
  const { live, sockets, log } = setup()
  sockets[0].frame({ type: 'hello', epoch: 'e1', seq: 0, online: [] })
  sockets[0].onclose?.({ code: 4401 })
  await new Promise((resolve) => setTimeout(resolve, 20))
  expect(sockets.length).toBe(1)
  expect(log.at(-1)).toBe('reconnecting')
  live.close()
})
