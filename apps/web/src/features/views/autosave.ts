/**
 * Debounced, serialized background saves. A failed save retries with bounded exponential backoff
 * (never a short fixed timer; see .ai/lessons.md "API retry load"), and gives up after `maxAttempts`
 * until the next change. The caller keeps its local state either way.
 */
export interface AutosaveOptions<T = unknown> {
  debounceMs?: number
  baseRetryMs?: number
  maxRetryMs?: number
  /** Attempts per value, the first save included. */
  maxAttempts?: number
  /** Called with the value that was saved. */
  onSaved?: (value: T) => void
  /** Timer functions; tests pass a manual clock instead of patching global timers. */
  timers?: AutosaveTimers
}

export interface AutosaveTimers {
  set(callback: () => void, ms: number): unknown
  clear(handle: unknown): void
}

const globalTimers: AutosaveTimers = {
  set: (callback, ms) => setTimeout(callback, ms),
  clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
}

export interface Autosaver<T> {
  /** Save `value` after the debounce; replaces any value still waiting. */
  schedule(value: T): void
  /**
   * Start a waiting save now (e.g. on unmount). If a save is in flight, the waiting value goes out
   * right after it, even when `dispose()` is called in between.
   */
  flush(): void
  /** Stop timers and retries; an in-flight request still completes. */
  dispose(): void
}

export const AUTOSAVE_DEFAULTS = { debounceMs: 500, baseRetryMs: 1_000, maxRetryMs: 30_000, maxAttempts: 6 } as const

export function createAutosaver<T>(save: (value: T) => Promise<unknown>, options: AutosaveOptions<T> = {}): Autosaver<T> {
  const debounceMs = options.debounceMs ?? AUTOSAVE_DEFAULTS.debounceMs
  const baseRetryMs = options.baseRetryMs ?? AUTOSAVE_DEFAULTS.baseRetryMs
  const maxRetryMs = options.maxRetryMs ?? AUTOSAVE_DEFAULTS.maxRetryMs
  const maxAttempts = options.maxAttempts ?? AUTOSAVE_DEFAULTS.maxAttempts
  const timers = options.timers ?? globalTimers
  let timer: unknown
  let pending: { value: T } | undefined
  let saving = false
  let runAgain = false
  let failures = 0
  let disposed = false
  /** A flush asked for a value that has to wait for the in-flight save; it still goes out after dispose. */
  let flushRequested = false
  let inFlight: { value: T } | undefined

  const arm = (delay: number) => {
    if (disposed) return
    if (timer !== undefined) timers.clear(timer)
    timer = timers.set(run, delay)
  }

  function run() {
    timer = undefined
    if ((disposed && !flushRequested) || !pending) return
    // one request at a time, so an older state can never land after a newer one
    if (saving) {
      runAgain = true
      return
    }
    const current = pending
    saving = true
    inFlight = current
    flushRequested = false
    void save(current.value)
      .then(
        () => {
          failures = 0
          if (pending === current) pending = undefined
          options.onSaved?.(current.value)
        },
        () => {
          if (pending !== current) return // a newer value already has its own timer
          failures += 1
          if (failures < maxAttempts) arm(Math.min(baseRetryMs * 2 ** (failures - 1), maxRetryMs))
        },
      )
      .finally(() => {
        saving = false
        inFlight = undefined
        if (runAgain) {
          runAgain = false
          run()
        }
      })
  }

  return {
    schedule(value) {
      if (disposed) return
      pending = { value }
      failures = 0
      arm(debounceMs)
    },
    flush() {
      if (disposed) return
      if (timer !== undefined) timers.clear(timer)
      timer = undefined
      // nothing waiting, or only the value already in flight (its retry keeps the backoff)
      if (!pending || pending === inFlight) return
      flushRequested = true
      run()
    },
    dispose() {
      disposed = true
      if (timer !== undefined) timers.clear(timer)
      timer = undefined
    },
  }
}
