import type { ConnectionStatus } from './types'

/** The part of `WebSocket` the live socket uses, so a test can put its own in. */
export interface SocketLike {
  onopen: (() => void) | null
  onmessage: ((event: { data: unknown }) => void) | null
  onclose: ((event: { code: number }) => void) | null
  send(data: string): void
  close(): void
}

/** A member in the `hello` frame: somebody who does not show as offline. */
export interface HelloPresence {
  user_id: string
  status: 'online' | 'idle' | 'dnd'
  emoji: string | null
  text: string | null
  expires_at: string | null
}

export interface LiveSocketOptions {
  /** `ws(s)://…/api/v1/workspaces/{id}/live`, without a query. */
  url: string
  /** A numbered event or a signal (typing, presence) of a topic. */
  onEvent: (topic: string, event: unknown) => void
  /** The first frame of every connection: every member who does not show as offline, with their status. */
  onHello: (presence: HelloPresence[]) => void
  /** Events were missed and the server cannot replay them: fetch the data again. */
  onResync: () => void
  onStatus: (status: ConnectionStatus) => void
  createSocket?: (url: string) => SocketLike
  /** The first reconnect delay; it doubles up to 30 times this. Default 1000. */
  retryMs?: number
  /** A connection that is silent for this long is taken as lost (the server pings every 25 seconds). Default 45000. */
  silenceMs?: number
}

export interface LiveSocket {
  /** Sends a frame if the socket is open; a signal that cannot go out is dropped. */
  send(frame: object): void
  /** The socket is connected and has delivered the event with this number (or a later one). */
  seen(seq: number): boolean
  /** Drops the connection and resolves when a new one is there. */
  reconnected(): Promise<void>
  close(): void
}

/** The session ended or the user lost access: reconnecting cannot help. */
const TERMINAL_CODES = new Set([4401, 4403])

/**
 * One connection to the workspace's live socket, kept open. After a drop it reconnects with the `epoch` and the number
 * of the last event it saw, so the server replays what was missed, or says `resync` when it cannot.
 */
export function openLiveSocket(options: LiveSocketOptions): LiveSocket {
  const createSocket = options.createSocket ?? ((url: string) => new WebSocket(url) as unknown as SocketLike)
  const retryMs = options.retryMs ?? 1000
  const silenceMs = options.silenceMs ?? 45_000
  let socket: SocketLike | null = null
  let closed = false
  let open = false
  let epoch: string | null = null
  /** The server's event number when this connection said hello; a resync moves `lastSeq` here. */
  let helloSeq = 0
  let lastSeq: number | null = null
  let attempt = 0
  let retry: ReturnType<typeof setTimeout> | undefined
  let silence: ReturnType<typeof setTimeout> | undefined
  let waiting: (() => void)[] = []

  const watch = () => {
    clearTimeout(silence)
    silence = setTimeout(() => socket?.close(), silenceMs)
  }

  const dropped = (code: number) => {
    clearTimeout(silence)
    socket = null
    if (closed) return
    open = false
    options.onStatus('reconnecting')
    if (TERMINAL_CODES.has(code)) return
    // 1x, 2x, 4x … 30x the base delay, with up to half of it added at random so tabs do not reconnect together.
    const delay = Math.min(retryMs * 2 ** attempt, retryMs * 30)
    attempt += 1
    retry = setTimeout(connect, delay * (1 + Math.random() / 2))
  }

  const receive = (data: unknown) => {
    watch()
    if (typeof data !== 'string') return
    let frame: { type?: string; epoch?: string; seq?: number; presence?: HelloPresence[]; topic?: string; event?: unknown }
    try {
      frame = JSON.parse(data)
    } catch {
      return
    }
    if (frame.type === 'hello') {
      // The numbers of a new epoch start again: the last number of the old one says nothing about it.
      if (frame.epoch !== epoch) lastSeq = null
      epoch = frame.epoch ?? null
      helloSeq = frame.seq ?? 0
      attempt = 0
      open = true
      options.onStatus('connected')
      options.onHello(frame.presence ?? [])
      for (const resolve of waiting.splice(0)) resolve()
    } else if (frame.type === 'resync') {
      lastSeq = helloSeq
      options.onResync()
    } else if (frame.topic !== undefined) {
      if (typeof frame.seq === 'number') lastSeq = frame.seq
      options.onEvent(frame.topic, frame.event)
    }
  }

  function connect() {
    if (closed) return
    const query = epoch !== null && lastSeq !== null ? `?epoch=${encodeURIComponent(epoch)}&after=${lastSeq}` : ''
    const current = createSocket(options.url + query)
    socket = current
    current.onopen = watch
    current.onmessage = (event) => receive(event.data)
    current.onclose = (event) => {
      if (socket === current) dropped(event.code)
    }
  }

  connect()
  return {
    send(frame) {
      if (open) socket?.send(JSON.stringify(frame))
    },
    seen: (seq) => open && lastSeq !== null && lastSeq >= seq,
    reconnected() {
      if (open) {
        // Not through `onclose`: on a dead network the browser reports the close only much later.
        const current = socket
        dropped(1006)
        current?.close()
      }
      return new Promise((resolve) => waiting.push(resolve))
    },
    close() {
      closed = true
      clearTimeout(retry)
      clearTimeout(silence)
      waiting = []
      socket?.close()
    },
  }
}
