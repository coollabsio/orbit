import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { ApiProblem } from '@/api/problem'
import { queryKeys } from '@/api/queryKeys'
import {
  savePreference,
  savedViewQueryOptions,
  useSavedView,
  useUpdateView,
  useViewPreference,
  viewStateOf,
  type SavedView,
} from '@/features/views/api/views'
import { createAutosaver, type Autosaver } from '@/features/views/autosave'
import {
  defaultViewState,
  effectiveFilter,
  legacyPreferencesToViewState,
  normalizeDisplay,
  normalizeViewState,
  viewStatesEqual,
  type DisplayOptions,
  type FilterGroup,
  type PageKey,
  type TaskPreset,
  type ViewState,
} from '@/features/views/viewState'

export type ViewSource =
  | { kind: 'page'; pageKey: PageKey; preset: TaskPreset | null; projectId: string | null }
  | { kind: 'view'; viewId: string }

export interface ViewStateController {
  state: ViewState
  effective: FilterGroup
  setFilter(filter: FilterGroup): void
  setDisplay(patch: Partial<DisplayOptions>): void
  reset(): void
  dirty: boolean
  canEdit: boolean
  view: SavedView | undefined
  stateError: string | null
  conflict: boolean
  save(opts?: { overwrite?: boolean }): Promise<void>
  discard(): void
  isLoading: boolean
}

type PageSource = Extract<ViewSource, { kind: 'page' }>

const LEGACY_LAYOUT_KEY = 'orbit:task_layout'
const legacyPreferencesKey = (workspaceId: string) => `orbit:task_preferences:${workspaceId}`

/** Placeholder while a page's preference loads; never shown because `isLoading` is true. */
const LOADING_STATE = defaultViewState()

function readLegacyState(workspaceId: string): { found: boolean; state: ViewState | null } {
  const rawPreferences = localStorage.getItem(legacyPreferencesKey(workspaceId))
  const layout = localStorage.getItem(LEGACY_LAYOUT_KEY)
  if (rawPreferences === null && layout === null) return { found: false, state: null }
  let preferences: unknown = null
  try {
    preferences = rawPreferences === null ? null : JSON.parse(rawPreferences)
  } catch {
    preferences = null
  }
  return { found: true, state: legacyPreferencesToViewState(preferences, layout) }
}

// the layout key was global, not per workspace: keep it so every workspace can still migrate it
function removeLegacyState(workspaceId: string) {
  localStorage.removeItem(legacyPreferencesKey(workspaceId))
}

/**
 * Unsaved saved-view edits for this session (spec §3): they survive navigating away and back, not a reload.
 * `baseVersion` is the view version the edits started from: Save checks against it, so a refetch under the
 * edits (e.g. another tab's save arriving over realtime) still ends in a conflict instead of an overwrite.
 */
type SessionEdit = { state: ViewState; baseVersion: number }
const sessionEdits = new Map<string, SessionEdit>()
const sessionListeners = new Set<() => void>()

function setSessionEdit(slot: string, edit: SessionEdit | undefined) {
  if (edit) sessionEdits.set(slot, edit)
  else sessionEdits.delete(slot)
  for (const listener of sessionListeners) listener()
}

function subscribeSessionEdits(listener: () => void) {
  sessionListeners.add(listener)
  return () => { sessionListeners.delete(listener) }
}

/** Drops every unsaved saved-view edit (sign-out, see `useLogout`; tests). */
export function clearViewSessionEdits() {
  sessionEdits.clear()
  for (const listener of sessionListeners) listener()
}

/**
 * Our own metadata update (Edit view: name, icon, visibility) moved the view from `fromVersion` to `toVersion`.
 * Unsaved edits that started from `fromVersion` now build on `toVersion`, so the next Save does not conflict with
 * ourselves. Edits based on any other version keep it: a change from another tab still conflicts.
 */
export function rebaseViewSessionEdit(workspaceId: string, viewId: string, fromVersion: number, toVersion: number) {
  const slot = `${workspaceId}|${viewId}`
  const edit = sessionEdits.get(slot)
  if (edit && edit.baseVersion === fromVersion) setSessionEdit(slot, { ...edit, baseVersion: toVersion })
}

export function useViewState(workspaceId: string, source: ViewSource): ViewStateController {
  const page = usePageViewState(workspaceId, source.kind === 'page' ? source : null)
  const saved = useSavedViewState(workspaceId, source.kind === 'view' ? source.viewId : undefined)
  return source.kind === 'page' ? page : saved
}

const noopSave = async () => {}
const noop = () => {}

function usePageViewState(workspaceId: string, source: PageSource | null): ViewStateController {
  const queryClient = useQueryClient()
  const pageKey: PageKey = source?.pageKey ?? 'all'
  const preference = useViewPreference(workspaceId, pageKey, source !== null)
  const slot = `${workspaceId}|${pageKey}`
  const [local, setLocal] = useState<{ slot: string; state: ViewState } | null>(null)
  const latest = useRef<{ slot: string; state: ViewState } | null>(null)
  const savers = useRef(new Map<string, Autosaver<ViewState>>())
  /** The newest state handed to each page's saver, to tell whether a finished save is still the latest. */
  const scheduled = useRef(new Map<string, ViewState>())
  const migrating = useRef(false)

  const saverFor = useCallback((key: PageKey) => {
    const id = `${workspaceId}|${key}`
    let saver = savers.current.get(id)
    if (!saver) {
      saver = createAutosaver((state: ViewState) => savePreference(workspaceId, key, state), {
        onSaved: (saved) => {
          // a refetch before the PUT landed (e.g. another cache invalidation) may have put the old server value in
          // the cache, and the page reseeds from it: put the saved state back unless a newer one is on its way
          if (scheduled.current.get(id) === saved) queryClient.setQueryData(queryKeys.viewPreference(workspaceId, key), saved)
          // the legacy keys go only once the migrated state is safely on the server
          if (key === 'all' && migrating.current) {
            migrating.current = false
            removeLegacyState(workspaceId)
          }
        },
      })
      savers.current.set(id, saver)
    }
    return saver
  }, [workspaceId, queryClient])

  const schedule = useCallback((key: PageKey, state: ViewState) => {
    scheduled.current.set(`${workspaceId}|${key}`, state)
    saverFor(key).schedule(state)
  }, [workspaceId, saverFor])

  // leaving the page sends any debounced change right away
  useEffect(() => {
    const all = savers.current
    return () => {
      for (const saver of all.values()) {
        saver.flush()
        saver.dispose()
      }
      all.clear()
    }
  }, [])

  // only a successful load seeds the page (a 404 already maps to `null`); after any other error the page stays
  // loading, so an edit can never autosave defaults over the stored preference
  const loaded = source !== null && preference.isSuccess
  const stored = preference.data
  useEffect(() => {
    if (!loaded || latest.current?.slot === slot) return
    let state = stored ? normalizeViewState(stored) : defaultViewState()
    if (stored === null && pageKey === 'all') {
      const legacy = readLegacyState(workspaceId)
      if (legacy.state) {
        state = legacy.state
        migrating.current = true
        queryClient.setQueryData(queryKeys.viewPreference(workspaceId, pageKey), state)
        schedule(pageKey, state)
      } else if (legacy.found) {
        removeLegacyState(workspaceId)
      }
    }
    latest.current = { slot, state }
    setLocal({ slot, state })
  }, [loaded, stored, slot, pageKey, workspaceId, queryClient, schedule])

  const ready = local !== null && local.slot === slot
  const state = ready ? local.state : LOADING_STATE
  const commit = (next: ViewState) => {
    if (latest.current?.slot !== slot) return
    latest.current = { slot, state: next }
    setLocal({ slot, state: next })
    // cache first, so coming back to this page starts from the newest state even before the PUT lands
    queryClient.setQueryData(queryKeys.viewPreference(workspaceId, pageKey), next)
    schedule(pageKey, next)
  }
  const current = () => (latest.current?.slot === slot ? latest.current.state : null)
  const preset = source?.preset ?? null
  const projectId = source?.projectId ?? null
  const effective = useMemo(() => effectiveFilter(state.filter, { preset, projectId }), [state.filter, preset, projectId])

  return {
    state,
    effective,
    setFilter: (filter) => {
      const now = current()
      if (now) commit({ ...now, filter })
    },
    setDisplay: (patch) => {
      const now = current()
      if (now) commit({ ...now, display: normalizeDisplay({ ...now.display, ...patch }) })
    },
    reset: () => commit(defaultViewState()),
    dirty: false,
    canEdit: true,
    view: undefined,
    stateError: null,
    conflict: false,
    save: noopSave,
    discard: noop,
    isLoading: source !== null && !ready,
  }
}

function useSavedViewState(workspaceId: string, viewId: string | undefined): ViewStateController {
  const queryClient = useQueryClient()
  const viewQuery = useSavedView(workspaceId, viewId)
  const updateView = useUpdateView(workspaceId)
  const slot = viewId ? `${workspaceId}|${viewId}` : ''
  const entry = useSyncExternalStore(subscribeSessionEdits, () => (slot ? sessionEdits.get(slot) : undefined))
  const edited = entry?.state
  const [conflictSlot, setConflictSlot] = useState<string | null>(null)
  // one save at a time: a second Save (e.g. Cmd+S pressed twice) would send the same base version and conflict
  const inFlight = useRef<Promise<void> | null>(null)
  const record = viewQuery.data
  const recordState = record ? viewStateOf(record) : null
  const base = useMemo(() => (recordState ? normalizeViewState(recordState) : defaultViewState()), [recordState])
  const stateError = record?.state_error ?? null
  const state = edited ?? base
  // an unreadable view counts as dirty, so Save can reset it ("Save to reset them")
  const dirty = record !== undefined && ((edited !== undefined && !viewStatesEqual(base, edited)) || stateError !== null)
  const current = () => (slot ? sessionEdits.get(slot)?.state : undefined) ?? base
  const edit = (next: ViewState) => {
    if (!slot || !record) return
    const started = sessionEdits.get(slot)
    // back at the saved state: nothing unsaved, and the next edit starts from the then-current version
    if (viewStatesEqual(next, base)) setSessionEdit(slot, undefined)
    else setSessionEdit(slot, { state: next, baseVersion: started?.baseVersion ?? record.version })
  }

  const discard = () => {
    if (!slot || !viewId) return
    setSessionEdit(slot, undefined)
    if (conflictSlot === slot) {
      // "Reload" in the conflict dialog: drop the edits and fetch the newer version
      setConflictSlot(null)
      void queryClient.invalidateQueries({ queryKey: queryKeys.view(workspaceId, viewId) })
    }
  }

  const save = (opts?: { overwrite?: boolean }): Promise<void> => {
    if (inFlight.current) return inFlight.current
    const run = saveOnce(opts).finally(() => { inFlight.current = null })
    inFlight.current = run
    return run
  }

  const saveOnce = async (opts?: { overwrite?: boolean }) => {
    if (!viewId || !record) return
    const pending = sessionEdits.get(slot)
    // an unreadable view has no edits but saves its defaults to reset it
    const body = pending ?? (stateError !== null ? { state: base, baseVersion: record.version } : undefined)
    if (!body) {
      // nothing left to save (e.g. the edits were undone): Overwrite still closes the conflict
      setConflictSlot(null)
      return
    }
    let version = body.baseVersion
    if (opts?.overwrite) {
      const fresh = await queryClient.fetchQuery({ ...savedViewQueryOptions(workspaceId, viewId), staleTime: 0 })
      version = fresh.version
    }
    let saved: SavedView
    try {
      saved = await updateView.mutateAsync({ viewId, body: { expected_version: version, state: body.state } })
    } catch (error) {
      if (error instanceof ApiProblem && error.status === 409) {
        setConflictSlot(slot)
        return
      }
      throw error
    }
    const latest = sessionEdits.get(slot)
    if (latest === pending) setSessionEdit(slot, undefined)
    // keep edits made while the request was in flight; they now build on the version this save produced
    else if (latest) setSessionEdit(slot, { ...latest, baseVersion: saved.version })
    setConflictSlot(null)
  }

  return {
    state,
    effective: effectiveFilter(state.filter, {}),
    setFilter: (filter) => edit({ ...current(), filter }),
    setDisplay: (patch) => {
      const now = current()
      edit({ ...now, display: normalizeDisplay({ ...now.display, ...patch }) })
    },
    reset: discard,
    dirty,
    canEdit: record?.can_edit ?? false,
    view: record,
    stateError,
    conflict: slot !== '' && conflictSlot === slot,
    save,
    discard,
    isLoading: viewId !== undefined && viewQuery.isPending,
  }
}
