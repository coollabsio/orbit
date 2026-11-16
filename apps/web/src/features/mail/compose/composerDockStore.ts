// The floating composers (§9.1 U2): which drafts are open as windows, minimized to pills, or full screen. At most
// three are open; one more minimizes the one opened longest ago. The list is kept in local storage for each workspace,
// so a reload keeps it (the drafts themselves are on the server). Pure state functions plus a small store for React.

export type DockWindowState = 'open' | 'minimized' | 'full'

export interface DockWindow {
  draftId: string
  state: DockWindowState
  /** When it was last opened, as a sequence number: the lowest is the oldest. */
  openedAt: number
}

/** Windows in the order they were first opened (their order on screen). */
export interface DockState {
  windows: readonly DockWindow[]
}

export const EMPTY_DOCK: DockState = { windows: [] }

/** Open windows (full screen counts) at the same time. */
export const MAX_OPEN_WINDOWS = 3

export const dockStorageKey = (workspaceId: string) => `orbit.mail.dock.${workspaceId}`

const isShown = (window: DockWindow) => window.state !== 'minimized'
const nextSequence = (state: DockState) => state.windows.reduce((max, window) => Math.max(max, window.openedAt), 0) + 1

/** Over the limit, the open windows opened longest ago are minimized (never `keep`, the one just opened). */
function enforceLimit(state: DockState, keep: string): DockState {
  const shown = state.windows.filter(isShown)
  if (shown.length <= MAX_OPEN_WINDOWS) return state
  const oldest = new Set(
    shown
      .filter((window) => window.draftId !== keep)
      .sort((a, b) => a.openedAt - b.openedAt)
      .slice(0, shown.length - MAX_OPEN_WINDOWS)
      .map((window) => window.draftId),
  )
  return { windows: state.windows.map((window) => (oldest.has(window.draftId) ? { ...window, state: 'minimized' } : window)) }
}

function show(state: DockState, draftId: string, as: 'open' | 'full'): DockState {
  const openedAt = nextSequence(state)
  const exists = state.windows.some((window) => window.draftId === draftId)
  const windows = exists
    ? state.windows.map((window) =>
        window.draftId === draftId
          ? { ...window, state: as, openedAt }
          : // one full screen window at a time: the one that was goes back to a window
            as === 'full' && window.state === 'full'
            ? { ...window, state: 'open' as const }
            : window,
      )
    : [...state.windows.map((window) => (as === 'full' && window.state === 'full' ? { ...window, state: 'open' as const } : window)), { draftId, state: as, openedAt }]
  return enforceLimit({ windows }, draftId)
}

/** Opens a draft as a window (a new one, a pill, or a full screen one going back to a window). */
export const openWindow = (state: DockState, draftId: string): DockState => show(state, draftId, 'open')

/** The inline reply moves to a window. */
export const popOutWindow = openWindow

/** Full screen (adds the draft when it is not in the dock). */
export const expandWindow = (state: DockState, draftId: string): DockState => show(state, draftId, 'full')

export function minimizeWindow(state: DockState, draftId: string): DockState {
  const window = state.windows.find((entry) => entry.draftId === draftId)
  if (!window || window.state === 'minimized') return state
  return { windows: state.windows.map((entry) => (entry === window ? { ...entry, state: 'minimized' } : entry)) }
}

export function closeWindow(state: DockState, draftId: string): DockState {
  if (!state.windows.some((window) => window.draftId === draftId)) return state
  return { windows: state.windows.filter((window) => window.draftId !== draftId) }
}

/**
 * Drops the windows of drafts that are gone (sent or deleted on another device). `existing` is the server's list of
 * the user's drafts; with `checked`, only those windows are looked at (the ones restored at load: a window opened
 * after the list was read is not in it).
 */
export function dropMissing(state: DockState, existing: ReadonlySet<string>, checked?: ReadonlySet<string>): DockState {
  const windows = state.windows.filter((window) => existing.has(window.draftId) || (checked !== undefined && !checked.has(window.draftId)))
  return windows.length === state.windows.length ? state : { windows }
}

/**
 * What the dock shows: the newest open windows that fit (`capacity`), in their order on screen; the rest, and the
 * minimized ones, as pills; the full screen one apart.
 */
export function layoutDock(state: DockState, capacity: number): { windows: string[]; pills: string[]; full: string | null } {
  const open = state.windows.filter((window) => window.state === 'open')
  const fits = new Set(
    [...open]
      .sort((a, b) => b.openedAt - a.openedAt)
      .slice(0, Math.max(1, capacity))
      .map((window) => window.draftId),
  )
  return {
    windows: open.filter((window) => fits.has(window.draftId)).map((window) => window.draftId),
    pills: state.windows.filter((window) => window.state === 'minimized' || (window.state === 'open' && !fits.has(window.draftId))).map((window) => window.draftId),
    full: state.windows.find((window) => window.state === 'full')?.draftId ?? null,
  }
}

/** Window width and the gap between windows, in px (the dock's CSS uses the same). */
export const DOCK_WINDOW_WIDTH = 520
export const DOCK_GAP = 12
const DOCK_MARGIN = 32

/** How many windows fit side by side in a viewport `width` px wide (1 to 3). */
export function dockCapacity(width: number): number {
  const fit = Math.floor((width - DOCK_MARGIN + DOCK_GAP) / (DOCK_WINDOW_WIDTH + DOCK_GAP))
  return Math.min(MAX_OPEN_WINDOWS, Math.max(1, fit))
}

// Local storage

const STORAGE_VERSION = 1
const STATES: readonly DockWindowState[] = ['open', 'minimized', 'full']

export function serializeDock(state: DockState): string {
  return JSON.stringify({ v: STORAGE_VERSION, windows: state.windows.map((window) => ({ id: window.draftId, state: window.state, at: window.openedAt })) })
}

/** The stored dock; anything unreadable is an empty dock, and unknown entries are skipped. */
export function parseDock(raw: string | null): DockState {
  if (!raw) return EMPTY_DOCK
  try {
    const value = JSON.parse(raw) as { v?: unknown; windows?: unknown }
    if (value?.v !== STORAGE_VERSION || !Array.isArray(value.windows)) return EMPTY_DOCK
    const windows: DockWindow[] = []
    for (const entry of value.windows as Array<{ id?: unknown; state?: unknown; at?: unknown }>) {
      if (typeof entry?.id !== 'string' || !entry.id || windows.some((window) => window.draftId === entry.id)) continue
      const state = STATES.includes(entry.state as DockWindowState) ? (entry.state as DockWindowState) : 'open'
      windows.push({ draftId: entry.id, state, openedAt: typeof entry.at === 'number' && Number.isFinite(entry.at) ? entry.at : 0 })
    }
    return { windows }
  } catch {
    return EMPTY_DOCK
  }
}

export type DockStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>

/** A dock for one workspace, saved to `storage` on every change. */
export function createComposerDockStore({ workspaceId, storage }: { workspaceId: string; storage: DockStorage | null }) {
  const key = dockStorageKey(workspaceId)
  const read = () => {
    try {
      return storage?.getItem(key) ?? null
    } catch {
      return null
    }
  }
  const restored = parseDock(read())
  // a reload does not cover the app with a full screen composer
  let state: DockState = { windows: restored.windows.map((window) => (window.state === 'full' ? { ...window, state: 'open' } : window)) }
  const restoredIds = restored.windows.map((window) => window.draftId)
  const listeners = new Set<() => void>()

  const set = (next: DockState) => {
    if (next === state) return
    state = next
    try {
      if (state.windows.length === 0) storage?.removeItem(key)
      else storage?.setItem(key, serializeDock(state))
    } catch {
      // storage full or blocked: the dock still works for this page
    }
    for (const listener of listeners) listener()
  }

  return {
    getSnapshot: () => state,
    subscribe: (listener: () => void) => {
      listeners.add(listener)
      return () => void listeners.delete(listener)
    },
    /** The drafts that were in the dock when it loaded. */
    restoredIds: () => restoredIds,
    has: (draftId: string) => state.windows.some((window) => window.draftId === draftId),
    open: (draftId: string) => set(openWindow(state, draftId)),
    popOut: (draftId: string) => set(popOutWindow(state, draftId)),
    expand: (draftId: string) => set(expandWindow(state, draftId)),
    minimize: (draftId: string) => set(minimizeWindow(state, draftId)),
    close: (draftId: string) => set(closeWindow(state, draftId)),
    /** Drops the restored windows whose drafts are not in `existing` (the user's drafts on the server). */
    dropMissing: (existing: Iterable<string>) => set(dropMissing(state, new Set(existing), new Set(restoredIds))),
  }
}

export type ComposerDockStore = ReturnType<typeof createComposerDockStore>
