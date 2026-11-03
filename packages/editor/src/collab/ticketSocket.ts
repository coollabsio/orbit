// A WebSocket for clients without the session cookie (the mobile app): each socket first fetches a fresh one-time
// ticket (`POST /api/v1/auth/socket-ticket`, valid 60 s) and opens the URL with `&ticket=…`. y-websocket creates a new
// socket for every connect and reconnect, so every attempt carries a new ticket.

/** What the ticket request returned; `status` 0 = no response (offline, timeout). */
export type TicketResult = { ok: true; ticket: string } | { ok: false; status: number }

/** The close code a failed ticket request reports to y-websocket. */
export function ticketFailureCode(status: number): number {
  // The session is gone: terminal (4401), like a socket the server closes for an ended session.
  if (status === 401) return 4401
  // Offline, a timeout or a server error: an abnormal close, which y-websocket retries with backoff.
  return 1006
}

export function withTicket(url: string, ticket: string): string {
  return `${url}${url.includes('?') ? '&' : '?'}ticket=${encodeURIComponent(ticket)}`
}

type Listener<E> = ((event: E) => void) | null

/** The part of the WebSocket constructor this wrapper uses (the browser's, or a fake in tests). */
export interface SocketConstructor {
  new (url: string, protocols?: string | string[]): WebSocket
}

/**
 * A WebSocket class (y-websocket's `WebSocketPolyfill`) that asks `getTicket` for a ticket before it opens the real
 * socket. Until then it reports CONNECTING; `close()` before that cancels the attempt.
 */
export function ticketWebSocket(getTicket: () => Promise<TicketResult>, Socket: SocketConstructor = WebSocket): typeof WebSocket {
  class TicketWebSocket {
    static readonly CONNECTING = 0
    static readonly OPEN = 1
    static readonly CLOSING = 2
    static readonly CLOSED = 3
    readonly CONNECTING = 0
    readonly OPEN = 1
    readonly CLOSING = 2
    readonly CLOSED = 3

    onopen: Listener<Event> = null
    onmessage: Listener<MessageEvent> = null
    onerror: Listener<Event> = null
    onclose: Listener<CloseEvent | { code: number; reason: string; wasClean: boolean }> = null

    private socket: WebSocket | null = null
    private cancelled = false
    private type: BinaryType = 'blob'

    constructor(url: string, protocols?: string | string[]) {
      getTicket().then(
        (result) => {
          if (this.cancelled) return
          if (!result.ok) return this.fail(ticketFailureCode(result.status), `socket ticket: HTTP ${result.status}`)
          const socket = new Socket(withTicket(url, result.ticket), protocols)
          socket.binaryType = this.type
          socket.onopen = (event) => this.onopen?.(event)
          socket.onmessage = (event) => this.onmessage?.(event)
          socket.onerror = (event) => this.onerror?.(event)
          socket.onclose = (event) => this.onclose?.(event)
          this.socket = socket
        },
        () => {
          if (!this.cancelled) this.fail(1006, 'socket ticket: no response')
        },
      )
    }

    get binaryType(): BinaryType {
      return this.socket?.binaryType ?? this.type
    }

    set binaryType(type: BinaryType) {
      this.type = type
      if (this.socket) this.socket.binaryType = type
    }

    get readyState(): number {
      if (this.socket) return this.socket.readyState
      return this.cancelled ? TicketWebSocket.CLOSED : TicketWebSocket.CONNECTING
    }

    send(data: string | ArrayBufferLike | Blob | ArrayBufferView) {
      if (!this.socket) throw new Error('The socket is not open yet.')
      this.socket.send(data as ArrayBuffer)
    }

    close(code?: number, reason?: string) {
      this.cancelled = true
      this.socket?.close(code, reason)
    }

    private fail(code: number, reason: string) {
      this.cancelled = true
      this.onclose?.({ code, reason, wasClean: false })
    }
  }
  return TicketWebSocket as unknown as typeof WebSocket
}
