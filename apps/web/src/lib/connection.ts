import { useSyncExternalStore } from 'react'

/**
 * Whether the app can reach the server, for the banner in the app shell. Each source says when it goes down and when
 * it is back; the browser being offline counts too. The store lives here, below the features, so the workspace events
 * socket, the chat socket and the shell can all use it without importing each other.
 */
export type ConnectionSource = 'events' | 'chat' | 'queries' | 'browser'

/** `lost`: reconnecting, a banner says so. `failed`: still down after a minute, "Orbit is unavailable" covers the app. */
export type ConnectionPhase = 'connected' | 'lost' | 'failed'

export interface ConnectionState {
  phase: ConnectionPhase
  /** When the first source went down (`Date.now()`); `null` while everything is up. */
  lostAt: number | null
}

export interface ConnectionStoreOptions {
  /** A drop shorter than this never shows. Default 1000. */
  showAfterMs?: number
  /** A drop longer than this is `failed`. Default 60000. */
  failAfterMs?: number
}

const CONNECTED: ConnectionState = { phase: 'connected', lostAt: null }

/** One store for the app (`connection` below); a test makes its own with short times. */
export function createConnectionStore(options: ConnectionStoreOptions = {}) {
  const showAfterMs = options.showAfterMs ?? 1000
  const failAfterMs = options.failAfterMs ?? 60_000
  const down = new Set<ConnectionSource>()
  const listeners = new Set<() => void>()
  const retryListeners = new Set<() => void>()
  let state = CONNECTED
  let showTimer: ReturnType<typeof setTimeout> | undefined
  let failTimer: ReturnType<typeof setTimeout> | undefined

  const set = (next: ConnectionState) => {
    state = next
    for (const listener of listeners) listener()
  }

  return {
    getState: () => state,
    subscribe(listener: () => void): () => void {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    /** A source is down (`true`) or back (`false`). The connection is lost while any source is down. */
    report(source: ConnectionSource, isDown: boolean) {
      if (down.has(source) === isDown) return
      if (isDown) down.add(source)
      else down.delete(source)
      if (down.size === 0) {
        clearTimeout(showTimer)
        clearTimeout(failTimer)
        set(CONNECTED)
      } else if (state.lostAt === null) {
        // Not shown yet: a blip that ends within `showAfterMs` stays `connected`.
        set({ phase: 'connected', lostAt: Date.now() })
        showTimer = setTimeout(() => {
          if (state.lostAt !== null && state.phase === 'connected') set({ phase: 'lost', lostAt: state.lostAt })
        }, showAfterMs)
        failTimer = setTimeout(() => {
          if (state.lostAt !== null) set({ phase: 'failed', lostAt: state.lostAt })
        }, failAfterMs)
      }
    },
    /** Runs when the user asks to retry: a socket reconnects at once, the shell fetches the core queries again. */
    onRetry(listener: () => void): () => void {
      retryListeners.add(listener)
      return () => retryListeners.delete(listener)
    },
    /** The Retry button: everything tries again now. The phase changes only when a source is back. */
    retry() {
      for (const listener of [...retryListeners]) listener()
    },
  }
}

export const connection = createConnectionStore()

if (typeof window !== 'undefined') {
  const browser = () => connection.report('browser', window.navigator.onLine === false)
  window.addEventListener('online', browser)
  window.addEventListener('offline', browser)
  browser()
}

export const reportConnection = connection.report
export const onConnectionRetry = connection.onRetry
export const retryConnection = connection.retry

/** The connection status of the app, with the time of the drop. */
export function useConnection(): ConnectionState {
  return useSyncExternalStore(connection.subscribe, connection.getState)
}

/**
 * Calls `reconnect` when a socket should skip the rest of its backoff wait: the browser is online again, the tab is
 * visible again, or the user pressed Retry. Returns the function that stops it.
 */
export function watchReconnect(reconnect: () => void): () => void {
  if (typeof window === 'undefined') return () => {}
  const visible = () => {
    if (document.visibilityState === 'visible') reconnect()
  }
  window.addEventListener('online', reconnect)
  document.addEventListener('visibilitychange', visible)
  const stop = onConnectionRetry(reconnect)
  return () => {
    window.removeEventListener('online', reconnect)
    document.removeEventListener('visibilitychange', visible)
    stop()
  }
}

/** The network failed or the server answered 5xx. A 4xx is an answer, not a lost connection. */
export function isConnectionFailure(error: unknown): boolean {
  const status = (error as { status?: unknown } | null)?.status
  return typeof status !== 'number' || status >= 500
}

/** `retry` of a core query (setup status, current user, workspaces): six more tries, and none after a 4xx. */
export function retryConnectionFailure(failureCount: number, error: unknown): boolean {
  return failureCount < 6 && isConnectionFailure(error)
}

/** `retryDelay` of a core query: 1, 2, 4, 8, 16, then 30 seconds. */
export function connectionRetryDelay(attempt: number): number {
  return Math.min(1000 * 2 ** attempt, 30_000)
}

interface QueryLike {
  data: unknown
  isError: boolean
  error: unknown
  failureCount: number
  failureReason: unknown
}

/** The last try of a query could not reach the server, whether it still retries or has given up. */
export function queryLostConnection(query: QueryLike): boolean {
  if (query.isError) return isConnectionFailure(query.error)
  return query.failureCount > 0 && isConnectionFailure(query.failureReason)
}

/**
 * A query has nothing good to show: it failed with no data, or the server refused it (a 4xx: access removed, record
 * deleted), which data from before must not hide. A lost connection with data from before is not this.
 */
export function loadFailed(query: Pick<QueryLike, 'data' | 'isError' | 'error'>): boolean {
  return query.isError && (query.data === undefined || !isConnectionFailure(query.error))
}

/**
 * The load of a gate does not work: `loadFailed`, or nothing to render and failed twice while it still retries. With
 * data from before, a refetch that lost the connection is not this: the app stays on screen and the banner tells the
 * user.
 */
export function firstLoadFailed(query: QueryLike): boolean {
  return loadFailed(query) || (query.data === undefined && query.failureCount > 1)
}
