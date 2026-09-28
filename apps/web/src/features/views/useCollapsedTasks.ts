import { useMemo, useState } from 'react'
import { readIds } from './useCollapsedGroups'

/** Collapsed sub-issue trees for one workspace; the list and the task detail share it. Expanded is the default. */
export function useCollapsedTasks(workspaceId: string): { collapsed: ReadonlySet<string>; toggle: (id: string) => void; expand: (id: string) => void } {
  const key = `orbit:task_tree_collapsed:${workspaceId}`
  const [ids, setIds] = useState<string[]>(() => readIds(key))
  const collapsed = useMemo(() => new Set(ids), [ids])
  const save = (next: string[]) => {
    setIds(next)
    try {
      window.localStorage.setItem(key, JSON.stringify(next))
    } catch {
      // storage full or blocked: the tree still toggles for this visit
    }
  }
  const toggle = (id: string) => save(ids.includes(id) ? ids.filter((item) => item !== id) : [...ids, id])
  const expand = (id: string) => {
    if (ids.includes(id)) save(ids.filter((item) => item !== id))
  }
  return { collapsed, toggle, expand }
}
