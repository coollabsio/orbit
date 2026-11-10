import { Calendar, Chart, ClipboardText, Gallery, Grid, Kanban, List, RowHorizontal } from 'reicon-react'
import type { DatabaseView } from '@orbit/database/types'

type ViewType = DatabaseView['type']

/** The "Add a new view" grid, in Notion's order. Entries without a `type` are not built yet. */
export const VIEW_KINDS: readonly { label: string; icon: typeof Grid; type: ViewType | null }[] = [
  { label: 'Table', icon: Grid, type: 'table' },
  { label: 'Board', icon: Kanban, type: 'board' },
  { label: 'List', icon: List, type: 'list' },
  { label: 'Gallery', icon: Gallery, type: 'gallery' },
  { label: 'Calendar', icon: Calendar, type: 'calendar' },
  { label: 'Timeline', icon: RowHorizontal, type: 'timeline' },
  { label: 'Chart', icon: Chart, type: null },
  { label: 'Form', icon: ClipboardText, type: 'form' },
]

export function viewIcon(type: ViewType): typeof Grid {
  return VIEW_KINDS.find((kind) => kind.type === type)?.icon ?? Grid
}

/** `base`, or `base 2`, `base 3`, … when a view already has the name. */
export function uniqueViewName(base: string, views: readonly DatabaseView[]): string {
  const names = new Set(views.map((view) => view.name))
  if (!names.has(base)) return base
  let n = 2
  while (names.has(`${base} ${n}`)) n++
  return `${base} ${n}`
}
