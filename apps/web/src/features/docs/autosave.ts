// Page autosave: debounced, single-flight, version-chained. Framework free so it can be tested with fake timers.
import type { Page, PageUpdateBody } from '@/api/generated/types.gen'
import { relativeTime } from '@/lib/format'

export type PagePatch = Omit<PageUpdateBody, 'expected_version'>

/**
 * - `idle`: nothing edited since the page opened
 * - `pending`: local edits wait for the debounce
 * - `saving`: a PATCH is in flight
 * - `saved`: every local edit is on the server
 * - `error`: the last save failed (edits kept; the next edit or `flush()` retries)
 * - `conflict`: the server has a newer version; saving stops until `overwrite()` or `discard()`
 */
export type SaveStatus = 'idle' | 'pending' | 'saving' | 'saved' | 'error' | 'conflict'

export interface AutosaveState {
  status: SaveStatus
  /** Set while `status === 'conflict'`: the server's version (and page, when the 409 body carried it). */
  conflict: { currentVersion: number | null; current: Page | null } | null
  error: unknown
}

export interface AutosaveTimers {
  set: (callback: () => void, ms: number) => unknown
  clear: (handle: unknown) => void
}

export interface PageAutosaverOptions {
  /** Version of the page as loaded; every save sends the latest known version as `expected_version`. */
  version: number
  save: (patch: PagePatch, expectedVersion: number) => Promise<Page>
  delay?: number
  onSaved?: (page: Page) => void
  onStateChange?: (state: AutosaveState) => void
  /** Classifies a failed save; conflicts carry the server's version and (optionally) its page. */
  conflictOf?: (error: unknown) => { currentVersion: number | null; current: Page | null } | null
  /**
   * Called on a conflict that carries the server page. Return true when that change does not touch what the local
   * copy edits (e.g. the page was moved): the save then retries on top of it instead of reporting a conflict.
   */
  rebaseOnto?: (current: Page) => boolean
  timers?: AutosaveTimers
}

export const AUTOSAVE_DELAY_MS = 800

const realTimers: AutosaveTimers = {
  set: (callback, ms) => setTimeout(callback, ms),
  clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
}

export class PageAutosaver {
  version: number
  state: AutosaveState = { status: 'idle', conflict: null, error: null }

  private pending: PagePatch | null = null
  private running: Promise<void> | null = null
  private again = false
  private timer: unknown = null
  private disposed = false
  private readonly options: PageAutosaverOptions
  private readonly timers: AutosaveTimers
  private readonly delay: number

  constructor(options: PageAutosaverOptions) {
    this.options = options
    this.version = options.version
    this.timers = options.timers ?? realTimers
    this.delay = options.delay ?? AUTOSAVE_DELAY_MS
  }

  /** Local edits that are not on the server yet (queued or in flight). */
  hasUnsavedChanges(): boolean {
    return this.pending !== null || this.running !== null
  }

  /** Records a local edit and (re)starts the debounce. During a conflict edits are kept but not sent. */
  update(patch: PagePatch): void {
    if (this.disposed) return
    this.pending = { ...this.pending, ...patch }
    if (this.state.status === 'conflict') return
    // Only notify on a status change: this runs on every keystroke.
    if (!this.running && this.state.status !== 'pending') this.setState({ status: 'pending', conflict: null, error: null })
    this.clearTimer()
    this.timer = this.timers.set(() => {
      this.timer = null
      void this.flush()
    }, this.delay)
  }

  /**
   * Saves queued edits now. Only one PATCH is ever in flight: a flush during a save marks the queue so it is sent
   * right after, with the version the running save returns. Resolves once the queue is empty or a save failed.
   */
  flush(): Promise<void> {
    this.clearTimer()
    if (this.running) {
      this.again = true
      return this.running
    }
    if (!this.pending || this.state.status === 'conflict' || this.disposed) return Promise.resolve()
    this.running = this.run()
    return this.running
  }

  /** Newer server version without content changes (e.g. the page was moved): keep editing on top of it. */
  adoptVersion(version: number): void {
    if (version > this.version) this.version = version
  }

  /** Conflict → "Overwrite": re-send `local` (the whole local page) on top of the server's current version. */
  overwrite(local: PagePatch, currentVersion?: number): Promise<void> {
    const version = currentVersion ?? this.state.conflict?.currentVersion ?? null
    if (version !== null) this.version = version
    this.pending = { ...this.pending, ...local }
    this.setState({ status: 'pending', conflict: null, error: null })
    return this.flush()
  }

  /** Conflict → "Reload": drop local edits and continue from the server page's version. */
  discard(serverVersion: number): void {
    this.clearTimer()
    this.pending = null
    this.version = serverVersion
    this.setState({ status: 'saved', conflict: null, error: null })
  }

  /** Stops scheduling and drops queued edits (the page was trashed). */
  dispose(): void {
    this.disposed = true
    this.clearTimer()
    this.pending = null
  }

  private async run(): Promise<void> {
    try {
      do {
        this.again = false
        const patch = this.pending
        if (!patch) break
        this.pending = null
        this.setState({ status: 'saving', conflict: null, error: null })
        try {
          const page = await this.options.save(patch, this.version)
          this.version = Math.max(this.version, page.version)
          this.options.onSaved?.(page)
        } catch (error) {
          // Keep the failed edits under anything typed while the request was out.
          // (TS narrows `pending` to null here; edits made during the request may have set it again.)
          const newer = this.pending as PagePatch | null
          this.pending = { ...patch, ...newer }
          const conflict = this.options.conflictOf?.(error) ?? null
          const current = conflict?.current
          // A strictly newer version is required, so this cannot loop forever.
          if (current && current.version > this.version && this.options.rebaseOnto?.(current)) {
            this.version = current.version
            this.again = true
            continue
          }
          this.setState(conflict ? { status: 'conflict', conflict, error } : { status: 'error', conflict: null, error })
          return
        }
      } while (this.again && this.pending)
      this.setState({ status: this.pending ? 'pending' : 'saved', conflict: null, error: null })
    } finally {
      this.running = null
    }
  }

  private clearTimer() {
    if (this.timer !== null) this.timers.clear(this.timer)
    this.timer = null
  }

  private setState(state: AutosaveState) {
    this.state = state
    this.options.onStateChange?.(state)
  }
}

/** Header label for the save state; `idle` falls back to when the page was last edited. */
export function saveStatusLabel(state: AutosaveState, updatedAt: string): string {
  switch (state.status) {
    case 'pending':
    case 'saving':
      return 'Saving…'
    case 'saved':
      return 'Saved'
    case 'error':
      return 'Save failed'
    case 'conflict':
      return 'Conflict'
    default:
      return `Edited ${relativeTime(updatedAt)}`
  }
}
