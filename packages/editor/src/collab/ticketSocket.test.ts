import { afterEach, describe, expect, test } from 'bun:test'
import { WebsocketProvider } from 'y-websocket'
import * as Y from 'yjs'
import { collabSocket } from './target'
import { ticketWebSocket, type TicketResult } from './ticketSocket'

class FakeSocket {
  static opened: FakeSocket[] = []
  readyState = 0
  binaryType = 'blob'
  onopen: ((event: unknown) => void) | null = null
  onmessage: ((event: unknown) => void) | null = null
  onerror: ((event: unknown) => void) | null = null
  onclose: ((event: unknown) => void) | null = null
  readonly url: string

  constructor(url: string) {
    this.url = url
    FakeSocket.opened.push(this)
  }

  send() {}

  close() {
    this.readyState = 3
  }

  /** The server ends the connection. */
  drop(code: number) {
    this.readyState = 3
    this.onclose?.({ code, reason: '', wasClean: false })
  }
}

const tick = (ms = 0) => new Promise((resolve) => setTimeout(resolve, ms))

async function until(check: () => boolean) {
  for (let i = 0; i < 100 && !check(); i++) await tick(5)
}

const target = { workspaceId: 'ws1', kind: 'pages' as const, id: 'p1', epoch: 'e1' }
let providers: WebsocketProvider[] = []

function provider(tickets: TicketResult[]) {
  const asked: number[] = []
  const getTicket = async () => {
    asked.push(asked.length)
    const next = tickets.shift()
    if (!next) throw new Error('no ticket left')
    return next
  }
  const { serverUrl, room, params } = collabSocket('https://orbit.test', target)
  const instance = new WebsocketProvider(serverUrl, room, new Y.Doc(), {
    params,
    disableBc: true,
    maxBackoffTime: 10,
    WebSocketPolyfill: ticketWebSocket(getTicket, FakeSocket as never),
  })
  providers.push(instance)
  return { instance, asked }
}

afterEach(() => {
  for (const instance of providers) instance.destroy()
  providers = []
  FakeSocket.opened = []
})

describe('ticketWebSocket', () => {
  test('every connection attempt opens the socket with a fresh ticket', async () => {
    provider([
      { ok: true, ticket: 'first' },
      { ok: true, ticket: 'sec/ond' },
    ])
    await until(() => FakeSocket.opened.length === 1)
    expect(FakeSocket.opened[0]!.url).toBe('wss://orbit.test/api/v1/workspaces/ws1/pages/p1/collab?v=1&epoch=e1&ticket=first')
    expect(FakeSocket.opened[0]!.binaryType).toBe('arraybuffer')

    FakeSocket.opened[0]!.drop(1006)
    await until(() => FakeSocket.opened.length === 2)
    expect(FakeSocket.opened[1]!.url).toBe('wss://orbit.test/api/v1/workspaces/ws1/pages/p1/collab?v=1&epoch=e1&ticket=sec%2Fond')
  })

  test('a 401 from the ticket request is a terminal 4401 close; nothing reconnects', async () => {
    const { instance, asked } = provider([{ ok: false, status: 401 }])
    const closed: number[] = []
    instance.on('closed', ({ code }: { code: number }) => closed.push(code))
    await until(() => closed.length === 1)
    await tick(50)
    expect(closed).toEqual([4401])
    expect(asked).toEqual([0])
    expect(FakeSocket.opened).toEqual([])
  })

  test('a failed ticket request (offline, 5xx) is retried with a new request', async () => {
    const { asked } = provider([
      { ok: false, status: 0 },
      { ok: false, status: 503 },
      { ok: true, ticket: 'third' },
    ])
    await until(() => FakeSocket.opened.length === 1)
    expect(asked).toEqual([0, 1, 2])
    expect(FakeSocket.opened.map((socket) => socket.url.split('ticket=')[1])).toEqual(['third'])
  })

  test('a connection closed before its ticket arrives never opens a socket', async () => {
    let release: (result: TicketResult) => void = () => {}
    const Socket = ticketWebSocket(() => new Promise((resolve) => (release = resolve)), FakeSocket as never)
    const socket = new Socket('wss://orbit.test/collab?v=1')
    expect(socket.readyState).toBe(0)
    socket.close()
    release({ ok: true, ticket: 'late' })
    await tick()
    expect(socket.readyState).toBe(3)
    expect(FakeSocket.opened).toEqual([])
  })
})
