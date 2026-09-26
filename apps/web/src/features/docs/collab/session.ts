// Co-editing session state for one page: wraps a y-websocket provider (or a test fake with the same shape),
// derives the header status, classifies terminal close codes and tracks local edits the server may not have yet.
// Framework free so it can be tested with a fake provider and fake timers.
import type { Awareness } from 'y-protocols/awareness'
import type * as Y from 'yjs'

/** The part of y-websocket's `WebsocketProvider` the app uses (a test fake implements the same). */
export interface CollabProviderLike {
  awareness: Awareness
  /** A SyncStep2 from the server arrived on the current socket. */
  readonly synced: boolean
  readonly wsconnected: boolean
  on(event: string, listener: (...args: never[]) => void): void
  off(event: string, listener: (...args: never[]) => void): void
  /** Resumes after a terminal close (4400–4499) or `disconnect()`. */
  connect(): void
  destroy(): void
}

export interface CollabConnection {
  doc: Y.Doc
  provider: CollabProviderLike
  /** `doc.getXmlFragment('prosemirror')`, what the editor binds to. */
  fragment: Y.XmlFragment
}

/**
 * - `connecting`: before the first sync (the editor is not mounted yet), or shortly after a drop
 * - `live`: connected and synced
 * - `offline`: disconnected for a while (or the browser is offline); y-websocket keeps retrying, edits stay local
 * - `reset`: the server's document was reset (4409) or the page was locked/unlocked (4423); the owner refetches the page
 *   and opens a new session with a fresh document (edits the server refused while locked are dropped)
 * - `lost`: no access any more (4403/4404) or the session ended (4401)
 * - `error`: the server refused the document for good (4413 too large, 4400/4426 protocol)
 */
export type CollabStatus = 'connecting' | 'live' | 'offline' | 'reset' | 'lost' | 'error'

/** Why the server closed the socket for good. */
export type CollabProblem = 'session' | 'forbidden' | 'gone' | 'reset' | 'too-large' | 'rate-limited' | 'protocol'

export interface CollabState {
  status: CollabStatus
  /** Set for terminal closes. */
  problem: CollabProblem | null
  /** The first sync happened: the document holds the server's state and the editor may bind to it. */
  ready: boolean
}

export interface CollabTimers {
  set: (callback: () => void, ms: number) => unknown
  clear: (handle: unknown) => void
  now: () => number
}

export interface CollabSessionOptions {
  onChange?: (state: CollabState) => void
  /** A terminal close; `rate-limited` reconnects by itself after `rateLimitRetryMs`. */
  onProblem?: (problem: CollabProblem) => void
  /**
   * Handshakes that fail before the upgrade (HTTP 401/403/404) reach the browser as close 1006 and y-websocket
   * retries them forever. After a few in a row the owner checks the page over REST (throttled here).
   */
  onProbe?: () => void
  timers?: CollabTimers
}

/** Close codes of `apps/server/src/collab/mod.rs` (`close`). */
export const CLOSE = {
  BAD_MESSAGE: 4400,
  SESSION: 4401,
  FORBIDDEN: 4403,
  GONE: 4404,
  RESET: 4409,
  /** The page was locked or unlocked: refetch it (lock state) and reconnect with a fresh document. */
  LOCK_CHANGED: 4423,
  TOO_LARGE: 4413,
  PROTOCOL: 4426,
  RATE_LIMITED: 4429,
  RESTART: 1012,
  OVERLOADED: 1013,
} as const

/** A drop shorter than this shows "Connecting…"; longer ones "Offline". */
export const OFFLINE_AFTER_MS = 2000
/** Edits this recent when the socket dropped may not have reached the server. */
export const UNSYNCED_WINDOW_MS = 3000
export const RATE_LIMIT_RETRY_MS = 5000
/** Failed handshakes in a row before the page is checked over REST, and the minimum gap between checks. */
export const PROBE_AFTER_FAILURES = 2
export const PROBE_INTERVAL_MS = 10_000

export function problemOfClose(code: number): CollabProblem | null {
  switch (code) {
    case CLOSE.SESSION:
      return 'session'
    case CLOSE.FORBIDDEN:
      return 'forbidden'
    case CLOSE.GONE:
      return 'gone'
    case CLOSE.RESET:
    case CLOSE.LOCK_CHANGED:
      return 'reset'
    case CLOSE.TOO_LARGE:
      return 'too-large'
    case CLOSE.RATE_LIMITED:
      return 'rate-limited'
    default:
      // Every other 4400–4499 code is terminal for y-websocket too (malformed frame, unsupported version).
      return code >= 4400 && code < 4500 ? 'protocol' : null
  }
}

const realTimers: CollabTimers = {
  set: (callback, ms) => setTimeout(callback, ms),
  clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
  now: () => Date.now(),
}

export class CollabSession {
  state: CollabState = { status: 'connecting', problem: null, ready: false }

  private readonly connection: CollabConnection
  private readonly options: CollabSessionOptions
  private readonly timers: CollabTimers
  private offlineTimer: unknown = null
  private retryTimer: unknown = null
  private dirty = false
  private lastLocalUpdate = Number.NEGATIVE_INFINITY
  private openedThisAttempt = false
  private failedHandshakes = 0
  private lastProbe = Number.NEGATIVE_INFINITY
  private browserOffline = false
  private destroyed = false
  private providerDestroyed = false
  private everSynced = false
  private readonly listeners: [string, (...args: never[]) => void][]
  private readonly onDocUpdate: (update: Uint8Array, origin: unknown) => void
  private readonly onBrowserOnline: () => void
  private readonly onBrowserOffline: () => void

  constructor(connection: CollabConnection, options: CollabSessionOptions = {}) {
    this.connection = connection
    this.options = options
    this.timers = options.timers ?? realTimers
    const { provider, doc } = connection

    const onStatus = ({ status }: { status: string }) => {
      if (status === 'connected') this.openedThisAttempt = true
      if (status === 'connecting') this.openedThisAttempt = false
      this.refresh()
    }
    const onSync = () => {
      if (provider.synced) {
        this.failedHandshakes = 0
        this.dirty = false
        this.everSynced = true
      }
      this.refresh()
    }
    const onConnectionClose = (event: { code?: number } | null) => {
      // Edits typed just before the drop may still have been in flight.
      if (this.timers.now() - this.lastLocalUpdate < UNSYNCED_WINDOW_MS) this.dirty = true
      if (!this.openedThisAttempt && (event?.code ?? 1006) === 1006) {
        this.failedHandshakes += 1
        this.maybeProbe()
      }
      this.openedThisAttempt = false
      this.refresh()
    }
    const onClosed = ({ code }: { code: number }) => {
      const problem = problemOfClose(code)
      if (!problem) return
      this.handleProblem(problem)
    }
    this.listeners = [
      ['status', onStatus as (...args: never[]) => void],
      ['sync', onSync as (...args: never[]) => void],
      ['connection-close', onConnectionClose as (...args: never[]) => void],
      ['closed', onClosed as (...args: never[]) => void],
    ]
    for (const [event, listener] of this.listeners) provider.on(event, listener)

    this.onDocUpdate = (_update, origin) => {
      if (origin === provider) return
      this.lastLocalUpdate = this.timers.now()
      if (this.state.status !== 'live') this.dirty = true
    }
    doc.on('update', this.onDocUpdate)

    this.onBrowserOnline = () => {
      this.browserOffline = false
      this.refresh()
    }
    this.onBrowserOffline = () => {
      this.browserOffline = true
      this.refresh()
    }
    if (typeof window !== 'undefined') {
      window.addEventListener('online', this.onBrowserOnline)
      window.addEventListener('offline', this.onBrowserOffline)
    }
    // The provider may have connected (or synced) before this session subscribed.
    if (provider.synced) onSync()
    else this.refresh()
  }

  /** Local edits the server may not have: typed while disconnected, or right before the connection dropped. */
  hasUnsyncedChanges(): boolean {
    return this.dirty && this.state.status !== 'live'
  }

  /** Ends the session: unsubscribes, destroys the provider (closes the socket) and the document. */
  destroy(): void {
    if (this.destroyed) return
    this.destroyed = true
    this.clearTimers()
    const { provider, doc } = this.connection
    for (const [event, listener] of this.listeners) provider.off(event, listener)
    doc.off('update', this.onDocUpdate)
    if (typeof window !== 'undefined') {
      window.removeEventListener('online', this.onBrowserOnline)
      window.removeEventListener('offline', this.onBrowserOffline)
    }
    this.destroyProvider()
    doc.destroy()
  }

  private destroyProvider() {
    if (this.providerDestroyed) return
    this.providerDestroyed = true
    this.connection.provider.destroy()
  }

  private handleProblem(problem: CollabProblem) {
    if (this.destroyed) return
    if (problem === 'rate-limited') {
      // Terminal for y-websocket; our own backoff resumes it. Edits stay in the document meanwhile.
      this.clearRetry()
      this.retryTimer = this.timers.set(() => {
        this.retryTimer = null
        if (!this.destroyed) this.connection.provider.connect()
      }, RATE_LIMIT_RETRY_MS)
      this.state = { ...this.state, problem }
      this.refresh()
      this.options.onProblem?.(problem)
      return
    }
    const status: CollabStatus =
      problem === 'reset' ? 'reset' : problem === 'too-large' || problem === 'protocol' ? 'error' : 'lost'
    this.clearTimers()
    this.setState({ ...this.state, status, problem })
    this.options.onProblem?.(problem)
  }

  /** Reports a problem found outside the socket (e.g. the REST probe saw a 404). */
  fail(problem: CollabProblem): void {
    if (this.state.problem && this.state.problem !== 'rate-limited') return
    // Stop y-websocket's retries: the page is not reachable any more.
    this.destroyProvider()
    this.handleProblem(problem)
  }

  private maybeProbe() {
    if (this.failedHandshakes < PROBE_AFTER_FAILURES) return
    const now = this.timers.now()
    if (now - this.lastProbe < PROBE_INTERVAL_MS) return
    this.lastProbe = now
    this.options.onProbe?.()
  }

  /** Recomputes the transient status (terminal problems other than rate limiting stick). */
  private refresh() {
    if (this.destroyed) return
    const terminal = this.state.problem && this.state.problem !== 'rate-limited'
    if (terminal) return
    const { provider } = this.connection
    const ready = this.everSynced
    const live = provider.wsconnected && provider.synced && !this.browserOffline
    if (live) {
      this.clearOfflineTimer()
      this.setState({ status: 'live', problem: null, ready })
      return
    }
    if (!ready) {
      this.setState({ ...this.state, status: this.browserOffline ? 'offline' : 'connecting', ready })
      return
    }
    if (this.browserOffline || this.state.problem === 'rate-limited') {
      this.clearOfflineTimer()
      this.setState({ ...this.state, status: 'offline', ready })
      return
    }
    if (this.state.status === 'live') {
      // A short blip reads "Connecting…"; if it lasts, "Offline".
      this.setState({ ...this.state, status: 'connecting', ready })
      this.clearOfflineTimer()
      this.offlineTimer = this.timers.set(() => {
        this.offlineTimer = null
        if (this.state.status === 'connecting' && this.state.ready) this.setState({ ...this.state, status: 'offline' })
      }, OFFLINE_AFTER_MS)
    }
  }

  private setState(state: CollabState) {
    const current = this.state
    if (current.status === state.status && current.problem === state.problem && current.ready === state.ready) return
    this.state = state
    this.options.onChange?.(state)
  }

  private clearOfflineTimer() {
    if (this.offlineTimer !== null) this.timers.clear(this.offlineTimer)
    this.offlineTimer = null
  }

  private clearRetry() {
    if (this.retryTimer !== null) this.timers.clear(this.retryTimer)
    this.retryTimer = null
  }

  private clearTimers() {
    this.clearOfflineTimer()
    this.clearRetry()
  }
}

/** Header label for the content sync state. */
export function collabStatusLabel(state: CollabState): string {
  switch (state.status) {
    case 'live':
      return 'Live'
    case 'offline':
      return 'Offline — changes will sync'
    case 'lost':
      return 'Access lost'
    case 'error':
      return 'Not syncing'
    case 'reset':
    case 'connecting':
    default:
      return 'Connecting…'
  }
}
