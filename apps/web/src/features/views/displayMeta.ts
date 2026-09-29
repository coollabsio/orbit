import { Calendar, Kanban, List, type IconComponent } from 'reicon-react'
import type { GroupBy, Layout, TaskProperty } from './viewState'

/** Names and icons for display options, shared by the Display popover and the Views page. */
export const LAYOUTS: Array<{ value: Layout; label: string; icon: IconComponent }> = [
  { value: 'list', label: 'List', icon: List },
  { value: 'board', label: 'Board', icon: Kanban },
  { value: 'timeline', label: 'Timeline', icon: Calendar },
]

export const GROUP_LABEL: Record<GroupBy, string> = {
  status: 'Status',
  assignee: 'Assignee',
  priority: 'Priority',
  project: 'Project',
  label: 'Label',
  none: 'No grouping',
}

/** The properties a timeline row has room for; the others keep their value for list and board. */
export const TIMELINE_PROPERTIES: readonly TaskProperty[] = ['id', 'status', 'assignee', 'priority']
