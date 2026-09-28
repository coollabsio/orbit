/**
 * The `ViewState` contract shared by saved views and per-page preferences (spec §1).
 * Pure data helpers: no React, no network.
 */

export type GroupOp = 'and' | 'or'
export type FilterField = 'status' | 'status_category' | 'assignee' | 'creator' | 'label' | 'priority' | 'project' | 'parent' | 'sub_issues' | 'due_date' | 'created_at' | 'updated_at' | 'text'
export type FilterOperator = 'is' | 'is_not' | 'is_empty' | 'is_not_empty' | 'includes_any' | 'includes_all' | 'excludes' | 'before' | 'after' | 'between' | 'contains'
export type DateValue = { absolute: string } | { relative: 'today' | 'start_of_week' | 'end_of_week'; offset_days?: number }
export type Condition = { field: FilterField; operator: FilterOperator; value?: unknown }
export type FilterGroup = { op: GroupOp; children: FilterNode[] }
export type FilterNode = FilterGroup | Condition
export type Layout = 'list' | 'board' | 'timeline'
export type GroupBy = 'status' | 'assignee' | 'priority' | 'project' | 'label' | 'none'
export type OrderBy = 'manual' | 'priority' | 'created' | 'updated' | 'title' | 'due_date'
export type TaskProperty = 'id' | 'status' | 'assignee' | 'priority' | 'project' | 'due_date' | 'labels' | 'created' | 'updated' | 'sub_issue_progress'
export type ShowCompleted = 'all' | 'past_week' | 'past_month' | 'none'
export type SubIssuesMode = 'nested' | 'flat' | 'hidden'
export type DisplayOptions = { layout: Layout; group_by: GroupBy; sub_group_by: GroupBy; order_by: OrderBy; order_direction: 'asc' | 'desc'; properties: TaskProperty[]; show_completed: ShowCompleted; show_empty_groups: boolean; sub_issues: SubIssuesMode }
export type ViewState = { filter: FilterGroup; display: DisplayOptions }
export type TaskPreset = 'mine' | 'overdue' | 'due_soon' | 'current_week' | 'my_week'
export type PageKey = 'all' | `project:${string}` | `preset:${TaskPreset}`

export const TASK_PRESETS: readonly TaskPreset[] = ['mine', 'overdue', 'due_soon', 'current_week', 'my_week']
/** Canonical property order; `normalizeDisplay` sorts `display.properties` into it. */
export const TASK_PROPERTIES: readonly TaskProperty[] = ['id', 'status', 'assignee', 'priority', 'project', 'due_date', 'labels', 'created', 'updated', 'sub_issue_progress']

export const DEFAULT_DISPLAY: DisplayOptions = {
  layout: 'list',
  group_by: 'status',
  sub_group_by: 'none',
  order_by: 'manual',
  order_direction: 'asc',
  properties: ['id', 'status', 'assignee', 'priority', 'project', 'due_date', 'labels', 'sub_issue_progress'],
  show_completed: 'all',
  show_empty_groups: false,
  sub_issues: 'nested',
}

export function emptyFilter(): FilterGroup {
  return { op: 'and', children: [] }
}

/** A fresh copy every call, so callers may mutate it. */
export function defaultViewState(): ViewState {
  return { filter: emptyFilter(), display: { ...DEFAULT_DISPLAY, properties: [...DEFAULT_DISPLAY.properties] } }
}

const notDone = (): Condition => ({ field: 'status_category', operator: 'is_not', value: ['completed', 'cancelled', 'duplicate'] })
const assignedToMe = (): Condition => ({ field: 'assignee', operator: 'is', value: ['me'] })
const dueThisWeek = (): Condition => ({ field: 'due_date', operator: 'between', value: [{ relative: 'start_of_week' }, { relative: 'end_of_week' }] })

/** Built-in pages as fixed filter trees (spec §1 "Built-in presets"). Treat as read-only. */
export const PRESET_FILTERS: Record<TaskPreset, FilterGroup> = {
  mine: { op: 'and', children: [assignedToMe()] },
  overdue: { op: 'and', children: [{ field: 'due_date', operator: 'before', value: { relative: 'today' } }, notDone()] },
  due_soon: { op: 'and', children: [{ field: 'due_date', operator: 'between', value: [{ relative: 'today' }, { relative: 'today', offset_days: 6 }] }, notDone()] },
  current_week: { op: 'and', children: [dueThisWeek(), notDone()] },
  my_week: { op: 'and', children: [dueThisWeek(), notDone(), assignedToMe()] },
}

export function isGroup(node: FilterNode): node is FilterGroup {
  return 'op' in node && Array.isArray((node as FilterGroup).children)
}

export function isTaskPreset(value: string | null | undefined): value is TaskPreset {
  return typeof value === 'string' && (TASK_PRESETS as readonly string[]).includes(value)
}

/** A preset page keeps its own preference even when the URL also carries `?project=`. */
export function pageKeyFor(input: { projectId?: string | null; preset?: TaskPreset | null }): PageKey {
  if (input.preset) return `preset:${input.preset}`
  if (input.projectId) return `project:${input.projectId}`
  return 'all'
}

/** Board needs a grouping; a sub-group never repeats the group; properties are a set in canonical order. */
export function normalizeDisplay(display: DisplayOptions): DisplayOptions {
  const group_by: GroupBy = display.layout === 'board' && display.group_by === 'none' ? 'status' : display.group_by
  const sub_group_by: GroupBy = group_by === 'none' || display.sub_group_by === group_by ? 'none' : display.sub_group_by
  const properties = TASK_PROPERTIES.filter((property) => display.properties.includes(property))
  return { ...display, group_by, sub_group_by, properties }
}

/** Server or migrated state → a complete, normalized state (missing display keys take defaults). */
export function normalizeViewState(state: ViewState): ViewState {
  return { filter: state.filter, display: normalizeDisplay({ ...DEFAULT_DISPLAY, ...state.display }) }
}

function canonicalDate(value: unknown): unknown {
  if (value && typeof value === 'object' && 'relative' in value) {
    const { relative, offset_days: offset } = value as { relative: string; offset_days?: number }
    return offset ? { relative, offset_days: offset } : { relative }
  }
  return value
}

function canonicalValue(operator: FilterOperator, value: unknown): unknown {
  if (value === undefined || value === null) return null
  if (operator === 'between' && Array.isArray(value)) return value.map(canonicalDate)
  if (Array.isArray(value) && value.every((item): item is string => typeof item === 'string')) return [...new Set(value)].sort()
  return canonicalDate(value)
}

function canonicalNode(node: FilterNode): unknown {
  if (isGroup(node)) return { op: node.op, children: node.children.map(canonicalNode) }
  return { field: node.field, operator: node.operator, value: canonicalValue(node.operator, node.value) }
}

function sortKeys(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortKeys)
  if (value && typeof value === 'object') {
    const record = value as Record<string, unknown>
    return Object.fromEntries(Object.keys(record).sort().map((key) => [key, sortKeys(record[key])]))
  }
  return value
}

function comparable(state: ViewState): string {
  return JSON.stringify(sortKeys({ filter: canonicalNode(state.filter), display: normalizeDisplay(state.display) }))
}

/**
 * Semantic equality used for the saved-view dirty flag.
 * Order-insensitive: `display.properties` and multi-value string lists (`is [a, b]` = `is [b, a]`).
 * Order-sensitive: sibling conditions/groups (the tree the user sees) and the `between` bounds.
 */
export function viewStatesEqual(a: ViewState, b: ViewState): boolean {
  return comparable(a) === comparable(b)
}

export function countConditions(group: FilterGroup): number {
  return group.children.reduce((total, child) => total + (isGroup(child) ? countConditions(child) : 1), 0)
}

/** True when the chip bar can show the tree: a root AND of plain conditions. */
export function isFlatFilter(group: FilterGroup): boolean {
  return group.op === 'and' && group.children.every((child) => !isGroup(child))
}

/**
 * AND of preset tree, project condition, and the user's tree — the body sent to the server.
 * A root-AND user tree is flattened into the result; a root-OR tree is nested as one child.
 * Without a scope the user tree is returned unchanged.
 */
export function effectiveFilter(user: FilterGroup, scope: { preset?: TaskPreset | null; projectId?: string | null }): FilterGroup {
  const children: FilterNode[] = []
  if (scope.preset) children.push(...PRESET_FILTERS[scope.preset].children)
  if (scope.projectId) children.push({ field: 'project', operator: 'is', value: [scope.projectId] })
  if (children.length === 0) return user
  if (user.children.length > 0) {
    if (user.op === 'and') children.push(...user.children)
    else children.push(user)
  }
  return { op: 'and', children }
}

const DAY_MS = 86_400_000

/** `YYYY-MM-DD` for a date value, resolved in UTC with weeks starting Monday (same rule as the server). */
export function resolveDateValue(value: DateValue, now: Date = new Date()): string {
  if ('absolute' in value) return value.absolute
  const today = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate())
  const sinceMonday = (new Date(today).getUTCDay() + 6) % 7
  const anchor = value.relative === 'today'
    ? today
    : value.relative === 'start_of_week'
      ? today - sinceMonday * DAY_MS
      : today + (6 - sinceMonday) * DAY_MS
  return new Date(anchor + (value.offset_days ?? 0) * DAY_MS).toISOString().slice(0, 10)
}

const LEGACY_SORT: Record<string, Pick<DisplayOptions, 'order_by' | 'order_direction'>> = {
  manual: { order_by: 'manual', order_direction: 'asc' },
  priority: { order_by: 'priority', order_direction: 'asc' },
  created: { order_by: 'created', order_direction: 'desc' },
  updated: { order_by: 'updated', order_direction: 'desc' },
  title: { order_by: 'title', order_direction: 'asc' },
}

/** The old `SortKey` → server ordering, keeping each key's old direction (see the removed `taskApiSort`). */
export function legacySortDisplay(sort: unknown): Pick<DisplayOptions, 'order_by' | 'order_direction'> {
  const order = typeof sort === 'string' && Object.hasOwn(LEGACY_SORT, sort) ? LEGACY_SORT[sort]! : LEGACY_SORT.manual!
  return { ...order }
}

const PRIORITIES = ['none', 'low', 'medium', 'high', 'urgent']

/**
 * One-time move of the old localStorage state (`orbit:task_preferences:<ws>` JSON + `orbit:task_layout`).
 * Single-value filters become root conditions; `unassignedFilter` wins over a stale `assigneeFilter`;
 * `searchFilter` is dropped (quick search is local and never saved). Returns null when neither key held anything.
 */
export function legacyPreferencesToViewState(prefs: unknown, layout: string | null): ViewState | null {
  const saved = prefs && typeof prefs === 'object' && !Array.isArray(prefs) ? prefs as Record<string, unknown> : null
  if (!saved && layout === null) return null
  const text = (value: unknown) => typeof value === 'string' && value.trim() !== '' ? value : null
  const children: Condition[] = []
  const status = text(saved?.statusFilter)
  if (status) children.push({ field: 'status', operator: 'is', value: [status] })
  const assignee = text(saved?.assigneeFilter)
  if (saved?.unassignedFilter === true) children.push({ field: 'assignee', operator: 'is_empty' })
  else if (assignee) children.push({ field: 'assignee', operator: 'is', value: [assignee] })
  const label = text(saved?.labelFilter)
  if (label) children.push({ field: 'label', operator: 'includes_any', value: [label] })
  const priority = saved?.priorityFilter
  if (typeof priority === 'string' && PRIORITIES.includes(priority)) children.push({ field: 'priority', operator: 'is', value: [priority] })
  const nextLayout: Layout = layout === 'board' || layout === 'timeline' ? layout : 'list'
  const display = normalizeDisplay({ ...defaultViewState().display, layout: nextLayout, ...legacySortDisplay(saved?.sort) })
  return { filter: { op: 'and', children }, display }
}
