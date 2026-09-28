import { useSyncExternalStore } from 'react'
import { readIds } from './useCollapsedGroups'

const listeners = new Set<() => void>()
/** Parsed ids per storage key, reused while the stored string is unchanged (stable snapshots). */
const snapshots = new Map<string, { raw: string | null; ids: ReadonlySet<string> }>()

function subscribe(listener: () => void) {
  listeners.add(listener)
  window.addEventListener('storage', listener)
  return () => {
    listeners.delete(listener)
    window.removeEventListener('storage', listener)
  }
}

function snapshot(key: string): ReadonlySet<string> {
  let raw: string | null = null
  try {
    raw = window.localStorage.getItem(key)
  } catch {
    // storage blocked: keep what this visit toggled
    return snapshots.get(key)?.ids ?? new Set()
  }
  const cached = snapshots.get(key)
  if (cached && cached.raw === raw) return cached.ids
  const ids = new Set(readIds(key))
  snapshots.set(key, { raw, ids })
  return ids
}

function save(key: string, ids: string[]) {
  const raw = JSON.stringify(ids)
  snapshots.set(key, { raw, ids: new Set(ids) })
  try {
    window.localStorage.setItem(key, raw)
  } catch {
    // storage full or blocked: the tree still toggles for this visit
  }
  for (const listener of listeners) listener()
}

/**
 * Collapsed sub-issue trees for one workspace; the list and the task detail share it (one store, so a toggle in
 * one never overwrites the other's). Expanded is the default.
 */
export function useCollapsedTasks(workspaceId: string): { collapsed: ReadonlySet<string>; toggle: (id: string) => void; expand: (id: string) => void } {
  const key = `orbit:task_tree_collapsed:${workspaceId}`
  const collapsed = useSyncExternalStore(subscribe, () => snapshot(key))
  const toggle = (id: string) => {
    const ids = [...snapshot(key)]
    save(key, ids.includes(id) ? ids.filter((item) => item !== id) : [...ids, id])
  }
  const expand = (id: string) => {
    const current = snapshot(key)
    if (current.has(id)) save(key, [...current].filter((item) => item !== id))
  }
  return { collapsed, toggle, expand }
}
