/** The part of `WebSocket` the app uses, so a test or a Bun script can put its own in. */
export interface SocketLike {
  onopen: (() => void) | null
  onmessage: ((event: { data: unknown }) => void) | null
  onclose: ((event: { code: number }) => void) | null
  close(): void
}

export interface ReconnectingSocketOptions {
  /** Read before every connection, so it can carry the resume cursor. */
  url: () => string
  create: (url: string) => SocketLike
  onMessage: (data: unknown) => void
  /** The server closed with 4401 or 4403: the session ended or access is gone. No retry follows. */
  onEnded?: () => void
  /** The first retry delay; it doubles up to 30 times this. Default 1000. */
  retryMs?: number
  /** Taken as lost when nothing arrives for this long (the server pings the live socket every 25 seconds). Off by default. */
  silenceMs?: number
}

export interface ReconnectingSocket {
  /** Drops the connection, if any, and connects at once, also after `close`: the app came back to the foreground. */
  reconnect(): void
  /** Closes the connection and stops retrying. */
  close(): void
}

const TERMINAL_CODES = new Set([4401, 4403])

/** One socket kept open: after a drop it waits 1x, 2x, 4x … 30x `retryMs` (plus up to half of it at random) and connects again. */
export function reconnectingSocket(options: ReconnectingSocketOptions): ReconnectingSocket {
  const retryMs = options.retryMs ?? 1000
  let socket: SocketLike | null = null
  let attempt = 0
  let stopped = false
  let retry: ReturnType<typeof setTimeout> | undefined
  let silence: ReturnType<typeof setTimeout> | undefined

  const watch = () => {
    if (options.silenceMs === undefined) return
    clearTimeout(silence)
    silence = setTimeout(() => socket?.close(), options.silenceMs)
  }

  /** Takes the handlers off the current socket, so its late close event changes nothing. */
  const detach = () => {
    clearTimeout(retry)
    clearTimeout(silence)
    const current = socket
    socket = null
    if (!current) return
    current.onopen = current.onmessage = current.onclose = null
    current.close()
  }

  function connect() {
    if (stopped) return
    const current = options.create(options.url())
    socket = current
    current.onopen = () => {
      attempt = 0
      watch()
    }
    current.onmessage = (event) => {
      watch()
      options.onMessage(event.data)
    }
    current.onclose = (event) => {
      if (socket !== current) return
      socket = null
      clearTimeout(silence)
      if (TERMINAL_CODES.has(event.code)) {
        stopped = true
        options.onEnded?.()
        return
      }
      const delay = Math.min(retryMs * 2 ** attempt, retryMs * 30)
      attempt += 1
      retry = setTimeout(connect, delay * (1 + Math.random() / 2))
    }
  }

  connect()
  return {
    reconnect() {
      detach()
      stopped = false
      attempt = 0
      connect()
    },
    close() {
      detach()
      stopped = true
    },
  }
}
