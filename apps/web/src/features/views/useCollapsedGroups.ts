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
  const toggle = (id: string) => setCollapsed((prev) => {
    const next = prev.includes(id) ? prev.filter((item) => item !== id) : [...prev, id]
    window.localStorage.setItem(storageKey, JSON.stringify(next))
    return next
  })
  return [collapsed, toggle]
}
