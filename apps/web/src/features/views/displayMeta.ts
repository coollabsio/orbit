import { Calendar, Kanban, List, type IconComponent } from 'reicon-react'
import type { TaskField } from '@orbit/api/generated/types.gen'
import type { DisplayOptions, GroupBy, Layout, TaskProperty } from './viewState'

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
  milestone: 'Milestone',
  cycle: 'Cycle',
  none: 'No grouping',
}

/** The properties a timeline row has room for; the others keep their value for list and board. */
export const TIMELINE_PROPERTIES: readonly TaskProperty[] = ['id', 'status', 'assignee', 'priority']

/**
 * The custom fields the Display popover offers as properties and orderings: on a project page the fields that project
 * shows (in its order), elsewhere the whole gallery (by name). A gallery field the display already shows or orders by
 * stays listed, so it can be turned off.
 */
export function displayFields(gallery: readonly TaskField[], projectFields: readonly TaskField[] | null, display: Pick<DisplayOptions, 'custom_fields' | 'order_field_id'>): TaskField[] {
  if (!projectFields) return [...gallery]
  const used = new Set([...(display.custom_fields ?? []), ...(display.order_field_id ? [display.order_field_id] : [])])
  const listed = new Set(projectFields.map((field) => field.id))
  return [...projectFields, ...gallery.filter((field) => used.has(field.id) && !listed.has(field.id))]
}

/** The shown custom fields as gallery records, in the display's order; a deleted field is skipped. */
export function shownCustomFields(gallery: readonly TaskField[], ids: readonly string[] | undefined): TaskField[] {
  if (!ids?.length) return []
  const byId = new Map(gallery.map((field) => [field.id, field]))
  return ids.flatMap((id) => byId.get(id) ?? [])
}
