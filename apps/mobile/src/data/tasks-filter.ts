/**
 * The task filter of the web's views (`apps/web/src/features/views/viewState.ts`, `filterFields.ts`): the filter tree,
 * the preset pages, the chip texts, and the server calls of a task page (`POST …/tasks/query`, the page preference).
 * No React, so the screen and the chip row share it.
 */
import { getViewPreference, putViewPreference, queryTasks } from '@orbit/api/generated/sdk.gen'
import type { TaskCycle, TaskField, TaskFieldKind, TaskMilestone, TaskQueryBody, ViewState as ApiViewState } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'

import type { IconName } from '../ui/Icon'
import type { LabelRecord, OrbitClient, ProjectRecord, StatusRecord, TaskRecord } from './orbit'
import { cycleLabel, cyclesForPicker, labelKeyOf, PRIORITY_LABEL, projectLabels, sortMilestones, sortStatuses, statusKeyOf, type Priority } from './tasks'

export type FilterField =
  | 'status'
  | 'status_category'
  | 'assignee'
  | 'creator'
  | 'label'
  | 'priority'
  | 'project'
  | 'parent'
  | 'sub_issues'
  | 'milestone'
  | 'cycle'
  | 'estimate'
  | 'due_date'
  | 'created_at'
  | 'updated_at'
  | 'text'
  | 'custom_field'
export type FilterOperator =
  | 'is'
  | 'is_not'
  | 'is_empty'
  | 'is_not_empty'
  | 'includes_any'
  | 'includes_all'
  | 'excludes'
  | 'before'
  | 'after'
  | 'between'
  | 'contains'
  | 'less_than'
  | 'greater_than'
export type DateValue = { absolute: string } | { relative: 'today' | 'start_of_week' | 'end_of_week'; offset_days?: number }
/** `field_id` names the task field of a `custom_field` condition, and only of one. */
export type Condition = { field: FilterField; field_id?: string; operator: FilterOperator; value?: unknown }
export type FilterGroup = { op: 'and' | 'or'; children: FilterNode[] }
export type FilterNode = FilterGroup | Condition
export type GroupBy = 'status' | 'assignee' | 'priority' | 'project' | 'label' | 'none'
export type DisplayOptions = {
  layout: 'list' | 'board' | 'timeline'
  group_by: GroupBy
  sub_group_by: GroupBy
  order_by: 'manual' | 'priority' | 'created' | 'updated' | 'title' | 'due_date' | 'estimate' | 'custom_field'
  /** The task field of `order_by: 'custom_field'`, set on the web; absent for every other ordering. */
  order_field_id?: string | null
  order_direction: 'asc' | 'desc'
  properties: string[]
  /** Task fields the web shows after `properties`; the app keeps them when it saves the state. */
  custom_fields?: string[]
  show_completed: 'all' | 'past_week' | 'past_month' | 'none'
  show_empty_groups: boolean
  sub_issues: 'nested' | 'flat' | 'hidden'
}
export type ViewState = { filter: FilterGroup; display: DisplayOptions }
export type TaskPreset = 'mine' | 'overdue' | 'due_soon' | 'current_week' | 'my_week'
export type PageKey = 'all' | `project:${string}` | `preset:${TaskPreset}`

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

export const emptyFilter = (): FilterGroup => ({ op: 'and', children: [] })

/** The preset pages' titles, which are also their chips' text. */
export const PRESET_LABEL: Record<TaskPreset, string> = {
  mine: 'My tasks',
  overdue: 'Overdue',
  due_soon: 'Due soon',
  current_week: 'This week',
  my_week: 'My week',
}

const notDone = (): Condition => ({ field: 'status_category', operator: 'is_not', value: ['completed', 'cancelled', 'duplicate'] })
const assignedToMe = (): Condition => ({ field: 'assignee', operator: 'is', value: ['me'] })
const dueThisWeek = (): Condition => ({ field: 'due_date', operator: 'between', value: [{ relative: 'start_of_week' }, { relative: 'end_of_week' }] })

const PRESET_FILTERS: Record<TaskPreset, FilterGroup> = {
  mine: { op: 'and', children: [assignedToMe()] },
  overdue: { op: 'and', children: [{ field: 'due_date', operator: 'before', value: { relative: 'today' } }, notDone()] },
  due_soon: { op: 'and', children: [{ field: 'due_date', operator: 'between', value: [{ relative: 'today' }, { relative: 'today', offset_days: 6 }] }, notDone()] },
  current_week: { op: 'and', children: [dueThisWeek(), notDone()] },
  my_week: { op: 'and', children: [dueThisWeek(), notDone(), assignedToMe()] },
}

export const isTaskPreset = (value: string | undefined): value is TaskPreset => value !== undefined && value in PRESET_LABEL

/** A preset page keeps its own preference even when the URL also carries `project`. */
export function pageKeyFor(preset: TaskPreset | undefined, projectId: string | null): PageKey {
  if (preset) return `preset:${preset}`
  if (projectId) return `project:${projectId}`
  return 'all'
}

const isGroup = (node: FilterNode): node is FilterGroup => 'op' in node && Array.isArray((node as FilterGroup).children)

export const countConditions = (group: FilterGroup): number => group.children.reduce((total, child) => total + (isGroup(child) ? countConditions(child) : 1), 0)

/** True when the chip row can show the tree: a root AND of plain conditions. */
export const isFlatFilter = (group: FilterGroup) => group.op === 'and' && group.children.every((child) => !isGroup(child))

/** AND of the preset tree, the project condition and the user's tree: the filter the server gets. */
export function effectiveFilter(user: FilterGroup, preset: TaskPreset | undefined, projectId: string | null): FilterGroup {
  const children: FilterNode[] = []
  if (preset) children.push(...PRESET_FILTERS[preset].children)
  if (projectId) children.push({ field: 'project', operator: 'is', value: [projectId] })
  if (children.length === 0) return user
  if (user.children.length > 0) {
    if (user.op === 'and') children.push(...user.children)
    else children.push(user)
  }
  return { op: 'and', children }
}

export function appendCondition(filter: FilterGroup, condition: Condition): FilterGroup {
  if (filter.op === 'and') return { ...filter, children: [...filter.children, condition] }
  if (filter.children.length <= 1) return { op: 'and', children: [...filter.children, condition] }
  return { op: 'and', children: [filter, condition] }
}

// ---- fields

/**
 * How the app edits a field: `list` and `date` pick values, `check` picks checked or not, `presence` picks "has a
 * value" or "no value" (estimates, and custom text, number and URL fields, whose typed values the app has no input
 * for). `text` and `task` chips (made on the web) can only be removed.
 */
type FieldKind = 'list' | 'date' | 'text' | 'task' | 'presence' | 'check'

export type FieldMeta = { label: string; plural: string; icon: IconName; kind: FieldKind; operators: FilterOperator[]; defaultOperator: FilterOperator }

export const FIELD_META: Record<FilterField, FieldMeta> = {
  status: { label: 'Status', plural: 'statuses', icon: 'circle', kind: 'list', operators: ['is', 'is_not'], defaultOperator: 'is' },
  status_category: { label: 'Status type', plural: 'status types', icon: 'category', kind: 'list', operators: ['is', 'is_not'], defaultOperator: 'is' },
  assignee: { label: 'Assignee', plural: 'assignees', icon: 'user', kind: 'list', operators: ['is', 'is_not', 'is_empty', 'is_not_empty'], defaultOperator: 'is' },
  creator: { label: 'Creator', plural: 'creators', icon: 'userEdit', kind: 'list', operators: ['is', 'is_not'], defaultOperator: 'is' },
  label: { label: 'Label', plural: 'labels', icon: 'tag', kind: 'list', operators: ['includes_any', 'includes_all', 'excludes', 'is_empty', 'is_not_empty'], defaultOperator: 'includes_any' },
  priority: { label: 'Priority', plural: 'priorities', icon: 'flag', kind: 'list', operators: ['is', 'is_not'], defaultOperator: 'is' },
  project: { label: 'Project', plural: 'projects', icon: 'folder', kind: 'list', operators: ['is', 'is_not'], defaultOperator: 'is' },
  parent: { label: 'Parent', plural: 'tasks', icon: 'subIssue', kind: 'task', operators: ['is', 'is_not'], defaultOperator: 'is' },
  sub_issues: { label: 'Sub-issues', plural: 'options', icon: 'subIssue', kind: 'list', operators: ['is'], defaultOperator: 'is' },
  milestone: { label: 'Milestone', plural: 'milestones', icon: 'target', kind: 'list', operators: ['is', 'is_not', 'is_empty', 'is_not_empty'], defaultOperator: 'is' },
  cycle: { label: 'Cycle', plural: 'cycles', icon: 'refresh', kind: 'list', operators: ['is', 'is_not', 'is_empty', 'is_not_empty'], defaultOperator: 'is' },
  // the server filters estimates only by presence
  estimate: { label: 'Estimate', plural: 'estimates', icon: 'chart', kind: 'presence', operators: ['is_not_empty', 'is_empty'], defaultOperator: 'is_not_empty' },
  due_date: { label: 'Due date', plural: 'dates', icon: 'calendar', kind: 'date', operators: ['before', 'after', 'between', 'is_empty', 'is_not_empty'], defaultOperator: 'before' },
  created_at: { label: 'Created', plural: 'dates', icon: 'calendarTick', kind: 'date', operators: ['before', 'after'], defaultOperator: 'after' },
  updated_at: { label: 'Updated', plural: 'dates', icon: 'clock', kind: 'date', operators: ['before', 'after'], defaultOperator: 'after' },
  text: { label: 'Text', plural: 'text', icon: 'text', kind: 'text', operators: ['contains'], defaultOperator: 'contains' },
  // a custom field the page has not loaded (or one deleted since): its chip shows and can be removed
  custom_field: { label: 'Field', plural: 'values', icon: 'text', kind: 'text', operators: [], defaultOperator: 'is_not_empty' },
}

const PRESENCE: FilterOperator[] = ['is_not_empty', 'is_empty']

/** A custom field's meta by kind (issue #87); `operators` are the ones the app can set. */
const CUSTOM_META: Record<TaskFieldKind, Omit<FieldMeta, 'label'>> = {
  text: { plural: 'values', icon: 'text', kind: 'presence', operators: PRESENCE, defaultOperator: 'is_not_empty' },
  url: { plural: 'values', icon: 'link', kind: 'presence', operators: PRESENCE, defaultOperator: 'is_not_empty' },
  number: { plural: 'values', icon: 'hashtag', kind: 'presence', operators: PRESENCE, defaultOperator: 'is_not_empty' },
  date: { plural: 'dates', icon: 'calendarDate', kind: 'date', operators: ['before', 'after', 'between', 'is_empty', 'is_not_empty'], defaultOperator: 'before' },
  select: { plural: 'options', icon: 'checkCircle', kind: 'list', operators: ['is', 'is_not', 'is_empty', 'is_not_empty'], defaultOperator: 'is' },
  multi_select: { plural: 'options', icon: 'list', kind: 'list', operators: ['includes_any', 'includes_all', 'excludes', 'is_empty', 'is_not_empty'], defaultOperator: 'includes_any' },
  checkbox: { plural: 'values', icon: 'checkSquare', kind: 'check', operators: ['is'], defaultOperator: 'is' },
}

/** What a condition filters on: a built-in field, or a custom field with its `field_id`. */
export type FilterTarget = Pick<Condition, 'field' | 'field_id'>

/** The target alone, without `field_id` for a built-in field (the server refuses it there). */
export const targetOf = (condition: FilterTarget): FilterTarget =>
  condition.field === 'custom_field' ? { field: 'custom_field', field_id: condition.field_id } : { field: condition.field }

export const customFieldOf = (target: FilterTarget, options: Pick<FilterOptions, 'customFields'>): TaskField | undefined =>
  target.field === 'custom_field' ? options.customFields.find((field) => field.id === target.field_id) : undefined

export function conditionMeta(target: FilterTarget, options: Pick<FilterOptions, 'customFields'>): FieldMeta {
  const custom = customFieldOf(target, options)
  return custom ? { label: custom.name, ...CUSTOM_META[custom.kind] } : FIELD_META[target.field]
}

/**
 * The fields the app can add, in the web's order. Parent (a task search) and Text (a typed value) need pickers the
 * app lacks; a chip of theirs from the web still shows and can be removed.
 */
export const FIELD_ORDER: FilterField[] = ['status', 'status_category', 'assignee', 'creator', 'label', 'priority', 'project', 'sub_issues', 'milestone', 'cycle', 'estimate', 'due_date', 'created_at', 'updated_at']

/** Milestones and cycles belong to one project, so only a project's page filters by them (as Linear does). */
const PROJECT_FIELDS: readonly FilterField[] = ['milestone', 'cycle', 'estimate']

export const fieldsFor = (projectId: string | null): FilterField[] => (projectId ? FIELD_ORDER : FIELD_ORDER.filter((field) => !PROJECT_FIELDS.includes(field)))

/** The "Filter by" list: the built-in fields, then the page's custom fields. */
export function filterTargets(options: Pick<FilterOptions, 'projectId' | 'customFields'>): FilterTarget[] {
  return [...fieldsFor(options.projectId).map((field): FilterTarget => ({ field })), ...options.customFields.map((field): FilterTarget => ({ field: 'custom_field', field_id: field.id }))]
}

/** A sheet row key for a target, and back. */
export const targetKey = (target: FilterTarget) => (target.field === 'custom_field' ? `custom:${target.field_id}` : target.field)
export const parseTargetKey = (key: string): FilterTarget => (key.startsWith('custom:') ? { field: 'custom_field', field_id: key.slice('custom:'.length) } : { field: key as FilterField })

/** A presence field's two choices, shown as values though they are operators. */
export function presenceChoices(target: FilterTarget): readonly { operator: FilterOperator; label: string }[] {
  return target.field === 'estimate'
    ? [
        { operator: 'is_not_empty', label: 'Has an estimate' },
        { operator: 'is_empty', label: 'No estimate' },
      ]
    : [
        { operator: 'is_not_empty', label: 'Has a value' },
        { operator: 'is_empty', label: 'No value' },
      ]
}

/** A checkbox field's two values. */
export const CHECK_CHOICES: readonly { value: boolean; label: string }[] = [
  { value: true, label: 'Checked' },
  { value: false, label: 'Not checked' },
]

/** User conditions per tree: the server allows 50 on the effective tree, and a scope adds up to 4. */
export const MAX_FILTER_CONDITIONS = 46

export const operatorNeedsValue = (operator: FilterOperator) => operator !== 'is_empty' && operator !== 'is_not_empty'

export function operatorLabel(operator: FilterOperator, count: number): string {
  switch (operator) {
    case 'is':
      return count > 1 ? 'is any of' : 'is'
    case 'is_not':
      return count > 1 ? 'is none of' : 'is not'
    case 'is_empty':
      return 'is empty'
    case 'is_not_empty':
      return 'is not empty'
    case 'includes_any':
      return count > 1 ? 'includes any of' : 'includes'
    case 'includes_all':
      return 'includes all of'
    case 'excludes':
      return 'excludes'
    case 'less_than':
      return 'less than'
    case 'greater_than':
      return 'greater than'
    case 'before':
    case 'after':
    case 'between':
    case 'contains':
      return operator
  }
}

export function defaultValue(meta: FieldMeta, operator: FilterOperator): unknown {
  if (!operatorNeedsValue(operator)) return undefined
  if (meta.kind === 'text') return ''
  if (meta.kind === 'check') return true
  if (meta.kind === 'date') return operator === 'between' ? [{ relative: 'start_of_week' }, { relative: 'end_of_week' }] : { relative: 'today' }
  return []
}

export function isDateValue(value: unknown): value is DateValue {
  if (!value || typeof value !== 'object') return false
  const candidate = value as Record<string, unknown>
  return typeof candidate.absolute === 'string' || candidate.relative === 'today' || candidate.relative === 'start_of_week' || candidate.relative === 'end_of_week'
}

export const listValue = (condition: Condition): string[] =>
  Array.isArray(condition.value) ? condition.value.filter((value): value is string => typeof value === 'string') : []

/** Changes the operator, converting the value where it can (date ↔ range) and clearing it where it must. */
export function withOperator(condition: Condition, operator: FilterOperator, meta: FieldMeta): Condition {
  const target = targetOf(condition)
  if (!operatorNeedsValue(operator)) return { ...target, operator }
  if (!operatorNeedsValue(condition.operator)) return { ...target, operator, value: defaultValue(meta, operator) }
  if (meta.kind === 'date') {
    const wasRange = condition.operator === 'between'
    if (operator === 'between' && !wasRange) return { ...target, operator, value: isDateValue(condition.value) ? [condition.value, condition.value] : defaultValue(meta, operator) }
    if (operator !== 'between' && wasRange) {
      const first = Array.isArray(condition.value) ? condition.value[0] : undefined
      return { ...target, operator, value: isDateValue(first) ? first : defaultValue(meta, operator) }
    }
  }
  return { ...condition, operator }
}

const isNumberPair = (value: unknown) => Array.isArray(value) && value.length === 2 && value.every((item) => typeof item === 'number')

/**
 * True when the server can evaluate it: list values present, text not blank, dates well-formed. A custom field's
 * value is checked by its operator, as the web may have set any of the kind's operators.
 */
export function isCompleteCondition(condition: Condition): boolean {
  if (condition.field === 'custom_field' && !condition.field_id) return false
  if (!operatorNeedsValue(condition.operator)) return true
  const { operator, value } = condition
  if (condition.field === 'custom_field') {
    switch (operator) {
      case 'before':
      case 'after':
        return isDateValue(value)
      case 'between':
        return isNumberPair(value) || (Array.isArray(value) && value.length === 2 && value.every(isDateValue))
      case 'less_than':
      case 'greater_than':
        return typeof value === 'number'
      case 'contains':
        return typeof value === 'string' && value.trim().length > 0
      case 'is':
        return typeof value === 'boolean' || typeof value === 'number' || listValue(condition).length > 0
      default:
        return listValue(condition).length > 0
    }
  }
  const kind = FIELD_META[condition.field].kind
  if (kind === 'text') return typeof value === 'string' && value.trim().length > 0
  if (kind === 'date') {
    if (operator !== 'between') return isDateValue(value)
    return Array.isArray(value) && value.length === 2 && value.every(isDateValue)
  }
  return listValue(condition).length > 0
}

// ---- values

/** Everything the pickers offer, loaded by the page. */
export type FilterOptions = {
  statuses: readonly StatusRecord[]
  members: readonly { id: string; name: string }[]
  labels: readonly LabelRecord[]
  /** A project page lists only this project's labels. */
  projectId: string | null
  projects: readonly ProjectRecord[]
  /** The project page's milestones and cycles; empty on other pages. */
  milestones: readonly TaskMilestone[]
  cycles: readonly TaskCycle[]
  /** The custom task fields the page filters by: the project's on a project page, else the workspace's gallery. */
  customFields: readonly TaskField[]
  currentUserId: string
}

export type Glyph =
  | { kind: 'status'; status: { category: string; color: string } }
  | { kind: 'dot'; color: string }
  | { kind: 'member'; id: string; name: string }
  | { kind: 'priority'; priority: Priority }
  | { kind: 'icon'; icon: IconName }

export type ValueOption = { value: string; label: string; glyph: Glyph }

const CATEGORY_LABEL: Record<string, string> = { unstarted: 'Unstarted', started: 'Started', completed: 'Completed', cancelled: 'Cancelled', duplicate: 'Duplicate' }
const PRIORITY_ORDER: Priority[] = ['urgent', 'high', 'medium', 'low', 'none']

export function valueOptions(target: FilterTarget, options: FilterOptions, mutedColor: string): ValueOption[] {
  switch (target.field) {
    case 'custom_field':
      return (customFieldOf(target, options)?.options ?? []).map((option) => ({ value: option.id, label: option.name, glyph: { kind: 'dot', color: option.color } }))
    case 'status': {
      const seen = new Set<string>()
      return sortStatuses(options.statuses).flatMap((status): ValueOption[] => {
        const key = statusKeyOf(status)
        if (seen.has(key)) return []
        seen.add(key)
        return [{ value: key, label: status.name, glyph: { kind: 'status', status } }]
      })
    }
    case 'status_category':
      return Object.entries(CATEGORY_LABEL).map(([category, label]) => ({ value: category, label, glyph: { kind: 'status', status: { category, color: mutedColor } } }))
    case 'assignee':
    case 'creator': {
      const me = options.members.find((person) => person.id === options.currentUserId)
      return [
        { value: 'me', label: 'Me', glyph: { kind: 'member', id: options.currentUserId, name: me?.name ?? 'Me' } },
        ...options.members.filter((person) => person.id !== options.currentUserId).map((person): ValueOption => ({ value: person.id, label: person.name, glyph: { kind: 'member', id: person.id, name: person.name } })),
      ]
    }
    case 'label': {
      // one option per key (the name): the server matches it in every project
      const byKey = new Map<string, LabelRecord>()
      for (const label of options.projectId ? projectLabels(options.labels, options.projectId) : options.labels) {
        const current = byKey.get(labelKeyOf(label))
        if (!current || label.id < current.id) byKey.set(labelKeyOf(label), label)
      }
      return [...byKey.values()].sort((a, b) => a.name.localeCompare(b.name)).map((label) => ({ value: labelKeyOf(label), label: label.name, glyph: { kind: 'dot', color: label.color } }))
    }
    case 'priority':
      return PRIORITY_ORDER.map((priority) => ({ value: priority, label: PRIORITY_LABEL[priority], glyph: { kind: 'priority', priority } }))
    case 'project':
      return [...options.projects].sort((a, b) => a.name.localeCompare(b.name)).map((project) => ({ value: project.id, label: project.name, glyph: { kind: 'dot', color: project.color } }))
    case 'milestone':
      return sortMilestones(options.milestones).map((milestone) => ({ value: milestone.id, label: milestone.name, glyph: { kind: 'icon', icon: 'target' } }))
    case 'cycle':
      return [
        { value: 'current', label: 'Current cycle', glyph: { kind: 'icon', icon: 'refresh' } },
        { value: 'next', label: 'Next cycle', glyph: { kind: 'icon', icon: 'refresh' } },
        { value: 'previous', label: 'Previous cycle', glyph: { kind: 'icon', icon: 'refresh' } },
        ...cyclesForPicker(options.cycles, new Date()).map((cycle): ValueOption => ({ value: cycle.id, label: cycleLabel(cycle), glyph: { kind: 'icon', icon: 'refresh' } })),
      ]
    case 'sub_issues':
      return [
        { value: 'has', label: 'Has sub-issues', glyph: { kind: 'icon', icon: 'subIssue' } },
        { value: 'none', label: 'No sub-issues', glyph: { kind: 'icon', icon: 'tasks' } },
      ]
    default:
      return []
  }
}

/** The list values as the pickers show them: a label id (older saved filters) reads as its label's key. */
export function chosenValues(condition: Condition, options: FilterOptions): string[] {
  const values = listValue(condition)
  if (condition.field !== 'label') return values
  const keys = new Map(options.labels.map((label) => [label.id, labelKeyOf(label)]))
  return [...new Set(values.map((value) => keys.get(value) ?? value))]
}

/** The chosen list values as options, in the order they were picked. Unknown IDs are dropped. */
export function selectedOptions(condition: Condition, options: FilterOptions, mutedColor: string): ValueOption[] {
  if (conditionMeta(condition, options).kind !== 'list') return []
  const known = new Map(valueOptions(condition, options, mutedColor).map((option) => [option.value, option]))
  return chosenValues(condition, options).flatMap((value): ValueOption[] => {
    const option = known.get(value)
    if (option) return [option]
    const person = options.members.find((candidate) => candidate.id === value)
    return person ? [{ value, label: person.name, glyph: { kind: 'member', id: person.id, name: person.name } }] : []
  })
}

export const SINGLE_DATE_PRESETS: Array<{ label: string; value: DateValue }> = [
  { label: 'Today', value: { relative: 'today' } },
  { label: 'Yesterday', value: { relative: 'today', offset_days: -1 } },
  { label: 'Tomorrow', value: { relative: 'today', offset_days: 1 } },
  { label: 'Start of week', value: { relative: 'start_of_week' } },
  { label: 'End of week', value: { relative: 'end_of_week' } },
  { label: '7 days ago', value: { relative: 'today', offset_days: -7 } },
  { label: 'In 7 days', value: { relative: 'today', offset_days: 7 } },
]

export const RANGE_DATE_PRESETS: Array<{ label: string; value: [DateValue, DateValue] }> = [
  { label: 'This week', value: [{ relative: 'start_of_week' }, { relative: 'end_of_week' }] },
  { label: 'Next week', value: [{ relative: 'start_of_week', offset_days: 7 }, { relative: 'end_of_week', offset_days: 7 }] },
  { label: 'Next 7 days', value: [{ relative: 'today' }, { relative: 'today', offset_days: 6 }] },
  { label: 'Last 7 days', value: [{ relative: 'today', offset_days: -6 }, { relative: 'today' }] },
]

const RELATIVE_LABEL = { today: 'Today', start_of_week: 'Start of week', end_of_week: 'End of week' } as const
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

export function dateValueLabel(value: DateValue): string {
  if ('absolute' in value) {
    const [year, month, day] = value.absolute.split('-').map(Number)
    return `${MONTHS[(month ?? 1) - 1]} ${day}, ${year}`
  }
  const offset = value.offset_days ?? 0
  if (value.relative === 'today') {
    if (offset === 0) return 'Today'
    if (offset === 1) return 'Tomorrow'
    if (offset === -1) return 'Yesterday'
    return offset > 0 ? `In ${offset} days` : `${-offset} days ago`
  }
  const base = RELATIVE_LABEL[value.relative]
  return offset === 0 ? base : `${base} ${offset > 0 ? '+' : '-'} ${Math.abs(offset)} days`
}

/** A value the app has no picker for (set on the web, or of a field the page has not loaded). */
function typedValueSummary(value: unknown): string {
  if (typeof value === 'string') return value ? `“${value}”` : ''
  if (typeof value === 'number') return value.toLocaleString()
  if (isNumberPair(value)) return (value as number[]).map((item) => item.toLocaleString()).join(' – ')
  if (isDateValue(value)) return dateValueLabel(value)
  if (Array.isArray(value) && value.length === 2 && value.every(isDateValue)) return `${dateValueLabel(value[0])} – ${dateValueLabel(value[1])}`
  if (Array.isArray(value) && value.length > 0) return value.length === 1 ? '1 value' : `${value.length} values`
  return ''
}

/** Chip value text: up to two names, then a count ("3 labels"). Empty for is_empty / is_not_empty. */
export function valueSummary(condition: Condition, options: FilterOptions, mutedColor: string): string {
  if (!operatorNeedsValue(condition.operator)) return ''
  const meta = conditionMeta(condition, options)
  if (meta.kind === 'check') return condition.value === true ? 'Checked' : 'Not checked'
  if (meta.kind === 'text' || meta.kind === 'presence') return typedValueSummary(condition.value)
  if (meta.kind === 'date') {
    if (condition.operator === 'between') {
      const range = Array.isArray(condition.value) ? condition.value : []
      return range.length === 2 && isDateValue(range[0]) && isDateValue(range[1]) ? `${dateValueLabel(range[0])} – ${dateValueLabel(range[1])}` : ''
    }
    return isDateValue(condition.value) ? dateValueLabel(condition.value) : ''
  }
  if (meta.kind === 'task') {
    const values = listValue(condition)
    return values.length > 2 ? `${values.length} tasks` : values.map((value) => (value === 'none' ? 'No parent' : `TASK-${value.slice(-4).toUpperCase()}`)).join(', ')
  }
  const values = chosenValues(condition, options)
  if (values.length > 2) return `${values.length} ${meta.plural}`
  const known = new Map(selectedOptions(condition, options, mutedColor).map((option) => [option.value, option.label]))
  return values.map((value) => known.get(value) ?? `Unknown ${meta.label.toLowerCase()}`).join(', ')
}

// ---- server

/** POST …/tasks/query body: the effective filter plus the display options the server applies. */
export function taskQueryBody(filter: FilterGroup, display: DisplayOptions): TaskQueryBody {
  return {
    filter: filter as unknown as TaskQueryBody['filter'],
    order_by: display.order_by === 'custom_field' && !display.order_field_id ? 'manual' : display.order_by,
    order_direction: display.order_direction,
    // the web may sort by a custom field; the server needs the field with that ordering, and refuses it without
    ...(display.order_by === 'custom_field' && display.order_field_id ? { order_field_id: display.order_field_id } : {}),
    show_completed: display.show_completed,
    ...(display.sub_issues === 'hidden' ? { sub_issues: 'hidden' as const } : {}),
  }
}

/** Every page of one task query, in server order. */
export async function queryAllTasks(client: OrbitClient, workspaceId: string, body: TaskQueryBody): Promise<TaskRecord[]> {
  const items: TaskRecord[] = []
  let cursor: string | undefined
  do {
    const { data } = await queryTasks({ client, path: { workspace_id: workspaceId }, body: { ...body, cursor, limit: 100 }, throwOnError: true })
    items.push(...data.items)
    cursor = data.next_cursor ?? undefined
  } while (cursor)
  return items
}

/** The page's stored filter and display (the web saves them per page), or null when the page has none yet. */
export async function viewPreference(client: OrbitClient, workspaceId: string, pageKey: PageKey): Promise<ViewState | null> {
  try {
    const { data } = await getViewPreference({ client, path: { workspace_id: workspaceId, page_key: pageKey }, throwOnError: true })
    const state = data.state as unknown as ViewState | null
    return state ? { filter: state.filter, display: { ...DEFAULT_DISPLAY, ...state.display } } : null
  } catch (error) {
    if (error instanceof ApiProblem && error.status === 404) return null
    throw error
  }
}

export async function saveViewPreference(client: OrbitClient, workspaceId: string, pageKey: PageKey, state: ViewState): Promise<void> {
  await putViewPreference({ client, path: { workspace_id: workspaceId, page_key: pageKey }, body: { state: state as unknown as ApiViewState }, throwOnError: true })
}
