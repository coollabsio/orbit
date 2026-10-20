import { afterEach } from 'bun:test'
import { cleanup } from '@testing-library/react'

afterEach(() => cleanup())

/**
 * No test talks to a server socket. happy-dom's WebSocket fails to connect with an unhandled error event, so
 * sockets stay "connecting" instead (screens with a co-editing document show their read-only preview). Tests that
 * need a live document pass a fake connection (`@/test/fakeCollab`).
 */
class IdleWebSocket extends EventTarget {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3
  readyState = IdleWebSocket.CONNECTING
  binaryType = 'blob'
  url: string
  onopen: ((event: Event) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  onclose: ((event: CloseEvent) => void) | null = null

  constructor(url: string | URL) {
    super()
    this.url = String(url)
  }

  send() {}

  close() {
    this.readyState = IdleWebSocket.CLOSED
  }
}
globalThis.WebSocket = IdleWebSocket as unknown as typeof WebSocket
