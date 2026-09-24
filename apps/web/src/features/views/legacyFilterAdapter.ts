import type { TaskPriority } from '@/features/tasks/api/models'
import type { SortKey } from '@/features/tasks/tasksLib'
import { isGroup, legacySortDisplay, type Condition, type DisplayOptions, type FilterGroup, type FilterNode, type FilterOperator } from '@/features/views/viewState'

/*
 * Interim bridge between the single-value `TaskFilters` menu and the ViewState filter tree.
 * Delete this file when FilterBar (Task 16) and DisplayPopover (Task 18) replace TaskFilters.
 */

export interface LegacyFilterValues {
  statusKey: string | null
  assigneeId: string | null
  unassigned: boolean
  labelId: string | null
  priority: TaskPriority | null
  sort: SortKey
}

export type LegacyFilterChange =
  | { field: 'status'; value: string | null }
  | { field: 'assignee'; value: string | null }
  | { field: 'unassigned'; value: boolean }
  | { field: 'label'; value: string | null }
  | { field: 'priority'; value: TaskPriority | null }

const rootConditions = (filter: FilterGroup): Condition[] =>
  filter.op === 'and' ? filter.children.filter((child): child is Condition => !isGroup(child)) : []

const single = (condition: Condition | undefined): string | null =>
  Array.isArray(condition?.value) && condition.value.length === 1 && typeof condition.value[0] === 'string' ? condition.value[0] : null

/** What the old menu shows as selected: single-value root conditions only. */
export function legacyFilterValues(filter: FilterGroup, display: DisplayOptions): LegacyFilterValues {
  const conditions = rootConditions(filter)
  const find = (field: Condition['field'], operator: FilterOperator) =>
    conditions.find((condition) => condition.field === field && condition.operator === operator)
  return {
    statusKey: single(find('status', 'is')),
    assigneeId: single(find('assignee', 'is')),
    unassigned: find('assignee', 'is_empty') !== undefined,
    labelId: single(find('label', 'includes_any')),
    priority: single(find('priority', 'is')) as TaskPriority | null,
    // the old menu has no due-date sort; it shows as unsorted
    sort: display.order_by === 'due_date' ? 'manual' : display.order_by,
  }
}

const REPLACES: Record<LegacyFilterChange['field'], { field: Condition['field']; operators: FilterOperator[] }> = {
  status: { field: 'status', operators: ['is'] },
  // picking a user (or clearing the user) also clears Unassigned: the old menu made them exclusive
  assignee: { field: 'assignee', operators: ['is', 'is_empty'] },
  unassigned: { field: 'assignee', operators: ['is_empty'] },
  label: { field: 'label', operators: ['includes_any'] },
  priority: { field: 'priority', operators: ['is'] },
}

function conditionFor(change: LegacyFilterChange): Condition | null {
  switch (change.field) {
    case 'status':
      return change.value ? { field: 'status', operator: 'is', value: [change.value] } : null
    case 'assignee':
      return change.value ? { field: 'assignee', operator: 'is', value: [change.value] } : null
    case 'unassigned':
      return change.value ? { field: 'assignee', operator: 'is_empty' } : null
    case 'label':
      return change.value ? { field: 'label', operator: 'includes_any', value: [change.value] } : null
    case 'priority':
      return change.value ? { field: 'priority', operator: 'is', value: [change.value] } : null
  }
}

/** Replaces the menu-owned root conditions for one field; nested groups and other conditions stay. */
export function withLegacyFilter(filter: FilterGroup, change: LegacyFilterChange): FilterGroup {
  const base: FilterGroup = filter.op === 'and' ? filter : { op: 'and', children: filter.children.length > 0 ? [filter] : [] }
  const { field, operators } = REPLACES[change.field]
  const kept: FilterNode[] = base.children.filter((child) => isGroup(child) || child.field !== field || !operators.includes(child.operator))
  const next = conditionFor(change)
  return { op: 'and', children: next ? [...kept, next] : kept }
}

export function withLegacySort(sort: SortKey): Pick<DisplayOptions, 'order_by' | 'order_direction'> {
  return legacySortDisplay(sort)
}
