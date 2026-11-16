import { applyDraftConflict, type DraftText } from '../mailLib'

export type DraftChange = DraftText

/** What a save ended with, when it did not save. */
export type DraftSaveProblem = 'sending' | 'error'

export interface DraftSaverState {
  saving: boolean
  problem: DraftSaveProblem | null
  /** Text the user typed that the server did not take (the "Restore my text" banner). */
  restore: DraftText | null
}

/** How a failed save reads: a newer version exists, the draft was sent, or anything else. */
export type DraftSaveFailure<D> = { kind: 'conflict'; current: D } | { kind: 'sending' } | { kind: 'other' }

export interface DraftSaverDeps<D extends DraftText & { version: number }> {
  /** The cached draft (its `version` is the one a save is made from); `undefined` once it is gone. */
  read: () => D | undefined
  write: (draft: D) => void
  send: (change: DraftChange, version: number) => Promise<D>
  classify: (error: unknown) => DraftSaveFailure<D>
  onState: (state: DraftSaverState) => void
  onSaved?: () => void
  /** Text still in `restore` when the saver is disposed: the owner keeps it for the next editor. */
  onDispose?: (restore: DraftText | null) => void
  /** Text kept by a saver of the same draft that was disposed. */
  initialRestore?: DraftText | null
  setTimer: (run: () => void, ms: number) => unknown
  clearTimer: (handle: unknown) => void
  delayMs: number
}

const isEmpty = (change: object) => Object.keys(change).length === 0

/**
 * Debounced autosave of one draft as a plain state machine (no React). `save` waits `delayMs` after the last call;
 * one save is in flight at a time and text typed meanwhile goes out after it. A conflict puts the server copy in the
 * cache and the user's text in `restore`; a `draft_sending` answer does the same and holds saving until `reset()`.
 * `stop()` ends saving for good (the draft was sent) until `reset()`; `dispose()` flushes what is left once and
 * nothing runs after it.
 */
export function createDraftSaver<D extends DraftText & { version: number }>(deps: DraftSaverDeps<D>) {
  let pending: DraftChange = {}
  let timer: unknown
  let running = false
  let held = false
  let halted = false
  let disposed = false
  const state: DraftSaverState = { saving: false, problem: null, restore: deps.initialRestore ?? null }

  const emit = () => deps.onState({ ...state })
  const clear = () => {
    if (timer !== undefined) deps.clearTimer(timer)
    timer = undefined
  }
  const arm = () => {
    clear()
    timer = deps.setTimer(() => {
      timer = undefined
      void run()
    }, deps.delayMs)
  }
  const keep = (text: DraftText | null) => {
    if (text) state.restore = { ...state.restore, ...text }
  }
  const blocked = () => disposed || held || halted

  /** The one save at unmount: fire and forget, no state, no timer. */
  function flushOnce() {
    const change = pending
    pending = {}
    const cached = deps.read()
    if (isEmpty(change) || !cached) return
    deps.send(change, cached.version).then(
      (draft) => {
        deps.write(draft)
        deps.onSaved?.()
      },
      () => {},
    )
  }

  async function run(): Promise<void> {
    if (blocked() || running || isEmpty(pending)) return
    const cached = deps.read()
    if (!cached) {
      // the draft is gone (sent or deleted): nothing to save, and not a failure
      pending = {}
      return
    }
    running = true
    state.saving = true
    emit()
    const change = pending
    pending = {}
    let failed = false
    try {
      deps.write(await deps.send(change, cached.version))
      state.problem = null
      deps.onSaved?.()
    } catch (error) {
      const failure = deps.classify(error)
      const unsaved = { ...change, ...pending }
      if (failure.kind === 'conflict') {
        deps.write(failure.current)
        keep(applyDraftConflict(unsaved, failure.current).restore)
        pending = {}
      } else if (failure.kind === 'sending') {
        keep(unsaved)
        pending = {}
        held = true
        state.problem = 'sending'
      } else {
        pending = unsaved
        failed = true
        state.problem = 'error'
      }
    } finally {
      running = false
      state.saving = false
    }
    if (disposed) {
      if (!failed && !held && !halted) flushOnce()
      return
    }
    emit()
    if (!isEmpty(pending) && !failed && !blocked()) arm()
  }

  function save(change: DraftChange) {
    if (disposed || halted) return
    if (held) {
      keep(change)
      emit()
      return
    }
    pending = { ...pending, ...change }
    arm()
  }

  return {
    save,
    /** Saves at once (as on close). */
    async flush() {
      clear()
      await run()
    },
    /** The draft was sent: no more saves, and what is pending is dropped. */
    stop() {
      halted = true
      pending = {}
      clear()
    },
    /**
     * Saving may go on: Undo returned the draft. Call it for that and nothing else (not for the saver's own writes
     * or a refetch of the draft).
     */
    reset() {
      if (disposed) return
      held = false
      halted = false
      if (state.problem === 'sending') {
        state.problem = null
        emit()
      }
    },
    applyRestore() {
      const text = state.restore
      if (!text) return
      state.restore = null
      emit()
      save(text)
    },
    dismissRestore() {
      state.restore = null
      emit()
    },
    dispose() {
      if (disposed) return
      disposed = true
      clear()
      if (!running && !held && !halted) flushOnce()
      deps.onDispose?.(state.restore)
    },
  }
}

export type DraftSaver = ReturnType<typeof createDraftSaver>

const undoListeners = new Map<string, Set<() => void>>()

/** Listens for the Undo of the draft with this key hash; returns the unsubscribe. */
export function onDraftUndone(hash: string, listener: () => void): () => void {
  let listeners = undoListeners.get(hash)
  if (!listeners) undoListeners.set(hash, (listeners = new Set()))
  listeners.add(listener)
  return () => void listeners.delete(listener)
}

/** Undo returned the draft: the savers of it go on. */
export function notifyDraftUndone(hash: string): void {
  for (const listener of undoListeners.get(hash) ?? []) listener()
}

const keptRestore = new Map<string, DraftText>()
/** Text a closed editor left in the "Restore my text" banner, until the next editor of that draft takes it. */
export const keepRestore = (hash: string, text: DraftText | null) => void (text ? keptRestore.set(hash, text) : keptRestore.delete(hash))
export const takeRestore = (hash: string): DraftText | null => {
  const text = keptRestore.get(hash) ?? null
  keptRestore.delete(hash)
  return text
}
