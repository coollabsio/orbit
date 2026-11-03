import { expect, test } from 'bun:test'

import { reconnectingSocket, type SocketLike } from './socket'

function fakes() {
  const opened: string[] = []
  const sockets: SocketLike[] = []
  const create = (url: string): SocketLike => {
    opened.push(url)
    const socket: SocketLike = { onopen: null, onmessage: null, onclose: null, close: () => socket.onclose?.({ code: 1000 }) }
    sockets.push(socket)
    return socket
  }
  return { opened, sockets, create }
}

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

test('reconnects after a drop with the current url, and stops on 4401', async () => {
  const { opened, sockets, create } = fakes()
  let cursor = 0
  const received: unknown[] = []
  let ended = false
  const live = reconnectingSocket({ url: () => `ws://x?after=${cursor}`, create, onMessage: (data) => received.push(data), onEnded: () => (ended = true), retryMs: 1 })
  sockets[0]!.onopen?.()
  sockets[0]!.onmessage?.({ data: 'one' })
  cursor = 7
  sockets[0]!.onclose?.({ code: 1006 })
  await wait(20)
  sockets[1]!.onclose?.({ code: 4401 })
  await wait(20)
  expect({ opened, received, ended }).toEqual({ opened: ['ws://x?after=0', 'ws://x?after=7'], received: ['one'], ended: true })
  live.close()
})

test('close stops retries; reconnect starts again at once', async () => {
  const { opened, sockets, create } = fakes()
  const live = reconnectingSocket({ url: () => 'ws://x', create, onMessage: () => {}, retryMs: 1 })
  live.close()
  sockets[0]!.onclose?.({ code: 1006 })
  await wait(20)
  expect(opened.length).toBe(1)
  live.reconnect()
  expect(opened.length).toBe(2)
  live.close()
})
