import { useState } from 'react'

function readIds(storageKey: string): string[] {
  try {
    const value = JSON.parse(window.localStorage.getItem(storageKey) ?? '[]')
    return Array.isArray(value) ? value.filter((id): id is string => typeof id === 'string') : []
  } catch {
    return []
  }
}

/**
 * Collapsed group ids for one page, remembered in localStorage (deliberately not part of the view state).
 * The caller keys the component by page, so a new `storageKey` always arrives with a fresh mount.
 */
export function useCollapsedGroups(storageKey: string): [string[], (id: string) => void] {
  const [collapsed, setCollapsed] = useState<string[]>(() => readIds(storageKey))
  const toggle = (id: string) => {
    const next = collapsed.includes(id) ? collapsed.filter((item) => item !== id) : [...collapsed, id]
    setCollapsed(next)
    try {
      window.localStorage.setItem(storageKey, JSON.stringify(next))
    } catch {
      // storage full or blocked: the group still toggles for this visit
    }
  }
  return [collapsed, toggle]
}
