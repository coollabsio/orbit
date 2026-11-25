import { format, parseISO } from 'date-fns'
import {
  Calendar,
  CalendarTick,
  Category,
  Clock,
  Danger,
  Diamonds,
  Flag,
  Folder,
  Hierarchy2,
  RecordCircle,
  RefreshCircle,
  Tag,
  TaskSquare,
  Text as TextIcon,
  Triangle,
  User as UserIcon,
  UserEdit,
  type IconComponent,
} from 'reicon-react'
import type { LabelRecord, ProjectRecord, TaskField, TaskFieldKind } from '@orbit/api/generated/types.gen'
import { FIELD_KIND_ICON } from '@/features/tasks/fieldsLib'
import type { Cycle, Milestone, TaskPriority, TaskStatusDef } from '@/features/tasks/api/models'
import { cycleName } from '@/features/tasks/planningLib'
import { CATEGORY_LABEL, CATEGORY_ORDER, labelKeyOf, labelRepresentatives, PRIORITY_LABEL, projectLabels, PRIORITY_ORDER, sortStatuses, statusKeyOf } from '@/features/tasks/taskMeta'
import type { User } from '@/features/workspaces/models'
import type { Condition, DateValue, FilterField, FilterOperator, TaskPreset } from './viewState'

export type TaskFilterRef = { identifier: string; title: string }

/** Everything the pickers can offer, loaded once by the page. */
export type FilterOptions = {
  statuses: TaskStatusDef[]
  members: User[]
  /** Every label; ids in saved filters resolve against all of them. */
  labels: LabelRecord[]
  /** A project page lists only this project's labels. */
  projectId?: string | null
  projects: ProjectRecord[]
  /** The page project's milestones and cycles (elsewhere those of the loaded tasks, so saved values keep their names). */
  milestones: Milestone[]
  cycles: Cycle[]
  currentUserId: string
  /** Parent filter values → task (null: deleted or trashed). Filled by `useFilterTaskRefs`. */
  taskRefs?: Readonly<Record<string, TaskFilterRef | null>>
  /** The workspace's custom task field gallery: each field is one more filter field. */
  taskFields: readonly TaskField[]
}

/** How a field's values are picked. `missing`: a custom field that was deleted (the condition can only be removed). */
export type FieldKind = 'list' | 'date' | 'text' | 'task' | 'number' | 'boolean' | 'missing'

/** The Parent filter's "no parent" value. */
export const NO_PARENT = 'none'

export type FieldMeta = {
  label: string
  plural: string
  icon: IconComponent
  kind: FieldKind
  operators: FilterOperator[]
  defaultOperator: FilterOperator
}

export type BuiltinFilterField = Exclude<FilterField, 'custom_field'>

export const FIELD_META: Record<BuiltinFilterField, FieldMeta> = {
  status: { label: 'Status', plural: 'statuses', icon: RecordCircle, kind: 'list', operators: ['is', 'is_not'], defaultOperator: 'is' },
  status_category: { label: 'Status type', plural: 'status types', icon: Category, kind: 'list', operators: ['is', 'is_not'], defaultOperator: 'is' },
  assignee: { label: 'Assignee', plural: 'assignees', icon: UserIcon, kind: 'list', operators: ['is', 'is_not', 'is_empty', 'is_not_empty'], defaultOperator: 'is' },
  creator: { label: 'Creator', plural: 'creators', icon: UserEdit, kind: 'list', operators: ['is', 'is_not'], defaultOperator: 'is' },
  label: { label: 'Label', plural: 'labels', icon: Tag, kind: 'list', operators: ['includes_any', 'includes_all', 'excludes', 'is_empty', 'is_not_empty'], defaultOperator: 'includes_any' },
  priority: { label: 'Priority', plural: 'priorities', icon: Flag, kind: 'list', operators: ['is', 'is_not'], defaultOperator: 'is' },
  project: { label: 'Project', plural: 'projects', icon: Folder, kind: 'list', operators: ['is', 'is_not'], defaultOperator: 'is' },
  parent: { label: 'Parent', plural: 'tasks', icon: Hierarchy2, kind: 'task', operators: ['is', 'is_not'], defaultOperator: 'is' },
  sub_issues: { label: 'Sub-issues', plural: 'options', icon: Hierarchy2, kind: 'list', operators: ['is'], defaultOperator: 'is' },
  due_date: { label: 'Due date', plural: 'dates', icon: Calendar, kind: 'date', operators: ['before', 'after', 'between', 'is_empty', 'is_not_empty'], defaultOperator: 'before' },
  created_at: { label: 'Created', plural: 'dates', icon: CalendarTick, kind: 'date', operators: ['before', 'after'], defaultOperator: 'after' },
  updated_at: { label: 'Updated', plural: 'dates', icon: Clock, kind: 'date', operators: ['before', 'after'], defaultOperator: 'after' },
  text: { label: 'Text', plural: 'text', icon: TextIcon, kind: 'text', operators: ['contains'], defaultOperator: 'contains' },
  milestone: { label: 'Milestone', plural: 'milestones', icon: Diamonds, kind: 'list', operators: ['is', 'is_not', 'is_empty', 'is_not_empty'], defaultOperator: 'is' },
  cycle: { label: 'Cycle', plural: 'cycles', icon: RefreshCircle, kind: 'list', operators: ['is', 'is_not', 'is_empty', 'is_not_empty'], defaultOperator: 'is' },
  // no values: a new Estimate filter joins at once as "is empty"
  estimate: { label: 'Estimate', plural: 'estimates', icon: Triangle, kind: 'list', operators: ['is_empty', 'is_not_empty'], defaultOperator: 'is_empty' },
}

export const FIELD_ORDER: BuiltinFilterField[] = ['status', 'status_category', 'assignee', 'creator', 'label', 'priority', 'project', 'milestone', 'cycle', 'estimate', 'parent', 'sub_issues', 'due_date', 'created_at', 'updated_at', 'text']

/** What a condition tests: a built-in field, or one custom field by id. */
export type FilterTarget = Pick<Condition, 'field' | 'field_id'>

/** Operators per custom field kind, as the server checks them; the first is the default. */
const CUSTOM_OPERATORS: Record<TaskFieldKind, { kind: FieldKind; operators: FilterOperator[] }> = {
  text: { kind: 'text', operators: ['contains', 'is_empty', 'is_not_empty'] },
  url: { kind: 'text', operators: ['contains', 'is_empty', 'is_not_empty'] },
  number: { kind: 'number', operators: ['is', 'less_than', 'greater_than', 'between', 'is_empty', 'is_not_empty'] },
  date: { kind: 'date', operators: ['before', 'after', 'between', 'is_empty', 'is_not_empty'] },
  select: { kind: 'list', operators: ['is', 'is_not', 'is_empty', 'is_not_empty'] },
  multi_select: { kind: 'list', operators: ['includes_any', 'includes_all', 'excludes', 'is_empty', 'is_not_empty'] },
  checkbox: { kind: 'boolean', operators: ['is'] },
}

export function customFieldMeta(field: Pick<TaskField, 'name' | 'kind'>): FieldMeta {
  const { kind, operators } = CUSTOM_OPERATORS[field.kind]
  return { label: field.name, plural: kind === 'list' ? 'options' : 'values', icon: FIELD_KIND_ICON[field.kind], kind, operators, defaultOperator: operators[0]! }
}

/** A condition on a deleted custom field: it reads as "no value" on the server, and the chip only offers removal. */
const MISSING_FIELD_META: FieldMeta = { label: 'Unknown field', plural: 'values', icon: Danger, kind: 'missing', operators: [], defaultOperator: 'is_empty' }

/** Names, icon and operators of a condition's field; a custom field's come from the gallery. */
export function fieldMeta(target: FilterTarget, options: Pick<FilterOptions, 'taskFields'>): FieldMeta {
  if (target.field !== 'custom_field') return FIELD_META[target.field]
  const field = options.taskFields.find((item) => item.id === target.field_id)
  return field ? customFieldMeta(field) : MISSING_FIELD_META
}

/** One row of a field picker. `key` is unique per row: the field name, or `custom_field:<id>`. */
export type FieldChoice = { key: string; target: FilterTarget; meta: FieldMeta }

export function targetKey(target: FilterTarget): string {
  return target.field === 'custom_field' ? `custom_field:${target.field_id ?? ''}` : target.field
}

/** The field part of a condition, without a stray `field_id` on a built-in field. */
export function targetOf(condition: FilterTarget): FilterTarget {
  return condition.field === 'custom_field' ? { field: 'custom_field', field_id: condition.field_id } : { field: condition.field }
}

/** Fields of one project's milestones and cycles: offered only on that project's page. */
const PROJECT_FIELDS: readonly FilterField[] = ['milestone', 'cycle']

/**
 * The fields a picker offers: the built-in ones, then every custom field of the gallery (a stored value counts even
 * where the task's project does not show the field). `keep` (the condition's own field) stays listed, so a saved
 * condition still reads.
 */
export function availableFields(options: FilterOptions, keep?: FilterTarget): FieldChoice[] {
  const builtin = FIELD_ORDER
    .filter((field) => options.projectId || field === keep?.field || !PROJECT_FIELDS.includes(field))
    .map((field): FieldChoice => ({ key: field, target: { field }, meta: FIELD_META[field] }))
  const custom = options.taskFields.map((field): FieldChoice => {
    const target: FilterTarget = { field: 'custom_field', field_id: field.id }
    return { key: targetKey(target), target, meta: customFieldMeta(field) }
  })
  const kept = keep?.field === 'custom_field' && !custom.some((choice) => choice.target.field_id === keep.field_id)
    ? [{ key: targetKey(keep), target: targetOf(keep), meta: MISSING_FIELD_META }]
    : []
  return [...builtin, ...custom, ...kept]
}

/** User conditions per tree: the server allows 50 on the effective tree, and a scope adds up to 3 preset conditions + 1 project condition. */
export const MAX_FILTER_CONDITIONS = 46

/** Values per condition, as the server allows. */
export const MAX_FILTER_VALUES = 100

/** Chip text for the non-removable preset scope. */
export const PRESET_LABEL: Record<TaskPreset, string> = {
  mine: 'My tasks',
  overdue: 'Overdue',
  due_soon: 'Due soon',
  current_week: 'This week',
  my_week: 'My week',
}

export type Glyph =
  | { kind: 'status'; status: Pick<TaskStatusDef, 'category' | 'color'> }
  | { kind: 'dot'; color: string }
  | { kind: 'member'; member: User | undefined; name: string }
  | { kind: 'priority'; priority: TaskPriority }
  | { kind: 'icon'; icon: IconComponent }

export type ValueOption = { value: string; label: string; glyph: Glyph }

/** Cycle filter values the server resolves per project, at query time. */
const RELATIVE_CYCLES = [
  { value: 'current', label: 'Current cycle' },
  { value: 'next', label: 'Next cycle' },
  { value: 'previous', label: 'Previous cycle' },
]

const byName = <T extends { name: string }>(a: T, b: T) => a.name.localeCompare(b.name)

/** One option per status key; same-named statuses of different projects share it. */
export function statusOptions(statuses: TaskStatusDef[]): ValueOption[] {
  const seen = new Set<string>()
  const options: ValueOption[] = []
  for (const status of sortStatuses(statuses)) {
    const key = statusKeyOf(status)
    if (seen.has(key)) continue
    seen.add(key)
    options.push({ value: key, label: status.name, glyph: { kind: 'status', status } })
  }
  return options
}

export function valueOptions(target: FilterTarget, options: FilterOptions): ValueOption[] {
  switch (target.field) {
    case 'status':
      return statusOptions(options.statuses)
    case 'status_category':
      return CATEGORY_ORDER.map((category): ValueOption => ({
        value: category,
        label: CATEGORY_LABEL[category],
        glyph: { kind: 'status', status: { category, color: 'var(--muted-foreground)' } },
      }))
    case 'assignee':
    case 'creator': {
      const me = options.members.find((person) => person.id === options.currentUserId)
      return [
        { value: 'me', label: 'Me', glyph: { kind: 'member', member: me, name: me?.name ?? 'Me' } },
        ...options.members
          .filter((person) => person.id !== options.currentUserId)
          .map((person): ValueOption => ({ value: person.id, label: person.name, glyph: { kind: 'member', member: person, name: person.name } })),
      ]
    }
    case 'label':
      // a label value is its key (the name): the server matches it in every project
      return labelRepresentatives(options.projectId ? projectLabels(options.labels, options.projectId) : options.labels).labels.map((item): ValueOption => ({ value: labelKeyOf(item), label: item.name, glyph: { kind: 'dot', color: item.color } }))
    case 'priority':
      return PRIORITY_ORDER.map((priority): ValueOption => ({ value: priority, label: PRIORITY_LABEL[priority], glyph: { kind: 'priority', priority } }))
    case 'project':
      return [...options.projects].sort(byName).map((project): ValueOption => ({ value: project.id, label: project.name, glyph: { kind: 'dot', color: project.color } }))
    case 'milestone':
      return [...options.milestones].sort((a, b) => a.position - b.position).map((milestone): ValueOption => ({ value: milestone.id, label: milestone.name, glyph: { kind: 'icon', icon: Diamonds } }))
    case 'cycle':
      return [
        ...RELATIVE_CYCLES.map((cycle): ValueOption => ({ ...cycle, glyph: { kind: 'icon', icon: RefreshCircle } })),
        ...[...options.cycles].sort((a, b) => a.starts_at.localeCompare(b.starts_at)).map((cycle): ValueOption => ({ value: cycle.id, label: cycleName(cycle), glyph: { kind: 'icon', icon: RefreshCircle } })),
      ]
    case 'sub_issues':
      return [
        { value: 'has', label: 'Has sub-issues', glyph: { kind: 'icon', icon: Hierarchy2 } },
        { value: 'none', label: 'No sub-issues', glyph: { kind: 'icon', icon: TaskSquare } },
      ]
    case 'custom_field':
      // in the field's own order, as rows and the task sidebar list them
      return (options.taskFields.find((field) => field.id === target.field_id)?.options ?? [])
        .map((option): ValueOption => ({ value: option.id, label: option.name, glyph: { kind: 'dot', color: option.color } }))
    default:
      return []
  }
}

export function listValue(condition: Condition): string[] {
  return Array.isArray(condition.value) ? condition.value.filter((value): value is string => typeof value === 'string') : []
}

/**
 * The list values as the pickers show them. A label id (views saved before labels were per project) reads as its
 * label's key, so it shows the label and editing the condition saves the key instead.
 */
export function chosenValues(condition: Condition, options: FilterOptions): string[] {
  const values = listValue(condition)
  if (condition.field !== 'label') return values
  const keys = new Map(options.labels.map((label) => [label.id, labelKeyOf(label)]))
  return [...new Set(values.map((value) => keys.get(value) ?? value))]
}

/** A member picked by ID rather than from the list, e.g. your own ID written by the legacy migration instead of `me`. */
function memberByIdOption(value: string, options: FilterOptions): ValueOption | undefined {
  const person = options.members.find((candidate) => candidate.id === value)
  return person ? { value, label: person.name, glyph: { kind: 'member', member: person, name: person.name } } : undefined
}

/** The chosen list values as options, in the order the user picked them. Unknown IDs are dropped. */
export function selectedOptions(condition: Condition, options: FilterOptions): ValueOption[] {
  if (fieldMeta(condition, options).kind !== 'list') return []
  const known = new Map(valueOptions(condition, options).map((option) => [option.value, option]))
  return chosenValues(condition, options).flatMap((value): ValueOption[] => {
    const option = known.get(value) ?? memberByIdOption(value, options)
    return option ? [option] : []
  })
}

/**
 * Chosen values the picker would not list: a member chosen by ID, or an ID that no longer exists ("Unknown label").
 * The picker shows them as extra rows, so they can still be seen and cleared.
 */
export function unlistedOptions(condition: Condition, options: FilterOptions): ValueOption[] {
  const meta = fieldMeta(condition, options)
  if (meta.kind !== 'list') return []
  const listed = new Set(valueOptions(condition, options).map((option) => option.value))
  return chosenValues(condition, options)
    .filter((value) => !listed.has(value))
    .map((value) => memberByIdOption(value, options) ?? { value, label: unknownValueLabel(condition, meta), glyph: { kind: 'dot', color: 'var(--muted-foreground)' } })
}

/** A list value that no longer exists: "Unknown label", or a custom field's "Removed option". */
function unknownValueLabel(condition: Condition, meta: FieldMeta): string {
  return condition.field === 'custom_field' ? 'Removed option' : `Unknown ${meta.label.toLowerCase()}`
}

export function isFilterNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value)
}

/** `1,500` style text of a number value. */
export function numberLabel(value: number): string {
  return value.toLocaleString()
}

export function isDateValue(value: unknown): value is DateValue {
  if (!value || typeof value !== 'object') return false
  const candidate = value as Record<string, unknown>
  return typeof candidate.absolute === 'string' || candidate.relative === 'today' || candidate.relative === 'start_of_week' || candidate.relative === 'end_of_week'
}

const RELATIVE_LABEL = { today: 'Today', start_of_week: 'Start of week', end_of_week: 'End of week' } as const

export function dateValueLabel(value: DateValue): string {
  if ('absolute' in value) return format(parseISO(value.absolute), 'MMM d, yyyy')
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

export function operatorNeedsValue(operator: FilterOperator): boolean {
  return operator !== 'is_empty' && operator !== 'is_not_empty'
}

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
    case 'before':
      return 'before'
    case 'after':
      return 'after'
    case 'between':
      return 'between'
    case 'contains':
      return 'contains'
    case 'less_than':
      return 'is less than'
    case 'greater_than':
      return 'is greater than'
  }
}

/** Chip and picker text for one Parent value. A task not loaded yet reads as its short id. */
export function taskValueLabel(value: string, options: FilterOptions): string {
  if (value === NO_PARENT) return 'No parent'
  const ref = options.taskRefs?.[value]
  if (ref === null) return 'Deleted task'
  return ref ? `${ref.identifier} ${ref.title || 'Untitled'}` : `TASK-${value.slice(-4).toUpperCase()}`
}

/** Chip value text: up to two names, then a count ("3 labels"). Empty for is_empty / is_not_empty. */
export function valueSummary(condition: Condition, options: FilterOptions): string {
  if (!operatorNeedsValue(condition.operator)) return ''
  const meta = fieldMeta(condition, options)
  if (meta.kind === 'missing') return ''
  if (meta.kind === 'boolean') return condition.value === true ? 'Checked' : condition.value === false ? 'Not checked' : ''
  if (meta.kind === 'number') {
    if (condition.operator !== 'between') return isFilterNumber(condition.value) ? numberLabel(condition.value) : ''
    const range = Array.isArray(condition.value) ? condition.value : []
    return range.length === 2 && isFilterNumber(range[0]) && isFilterNumber(range[1]) ? `${numberLabel(range[0])} – ${numberLabel(range[1])}` : ''
  }
  if (meta.kind === 'text') return typeof condition.value === 'string' && condition.value ? `“${condition.value}”` : ''
  if (meta.kind === 'date') {
    if (condition.operator === 'between') {
      const range = Array.isArray(condition.value) ? condition.value : []
      return range.length === 2 && isDateValue(range[0]) && isDateValue(range[1]) ? `${dateValueLabel(range[0])} – ${dateValueLabel(range[1])}` : ''
    }
    return isDateValue(condition.value) ? dateValueLabel(condition.value) : ''
  }
  if (meta.kind === 'task') {
    const values = listValue(condition)
    return values.length > 2 ? `${values.length} tasks` : values.map((value) => taskValueLabel(value, options)).join(', ')
  }
  const values = chosenValues(condition, options)
  if (values.length > 2) return `${values.length} ${meta.plural}`
  const known = new Map(selectedOptions(condition, options).map((option) => [option.value, option.label]))
  return values.map((value) => known.get(value) ?? unknownValueLabel(condition, meta)).join(', ')
}

/** The starting value of a new condition. Numbers and checkboxes start blank, so their picker asks for one. */
export function defaultValue(kind: FieldKind, operator: FilterOperator): unknown {
  if (!operatorNeedsValue(operator)) return undefined
  if (kind === 'text') return ''
  if (kind === 'date') return operator === 'between' ? [{ relative: 'start_of_week' }, { relative: 'end_of_week' }] : { relative: 'today' }
  if (kind === 'number' || kind === 'boolean' || kind === 'missing') return undefined
  return []
}

/** A new condition on a field, with its default operator and value. */
export function newCondition(target: FilterTarget, meta: FieldMeta): Condition {
  const operator = meta.defaultOperator
  return operatorNeedsValue(operator) ? { ...targetOf(target), operator, value: defaultValue(meta.kind, operator) } : { ...targetOf(target), operator }
}

/**
 * Changes the operator, converting the value where it can (a date or number ↔ a range of it) and clearing it where it
 * must.
 */
export function withOperator(condition: Condition, operator: FilterOperator, options: Pick<FilterOptions, 'taskFields'>): Condition {
  const target = targetOf(condition)
  const { kind } = fieldMeta(condition, options)
  if (!operatorNeedsValue(operator)) return { ...target, operator }
  if (!operatorNeedsValue(condition.operator)) return { ...target, operator, value: defaultValue(kind, operator) }
  if (kind === 'date' || kind === 'number') {
    const single = kind === 'date' ? isDateValue : isFilterNumber
    const wasRange = condition.operator === 'between'
    if (operator === 'between' && !wasRange) {
      return { ...target, operator, value: single(condition.value) ? [condition.value, condition.value] : defaultValue(kind, operator) }
    }
    if (operator !== 'between' && wasRange) {
      const first = Array.isArray(condition.value) ? condition.value[0] : undefined
      return { ...target, operator, value: single(first) ? first : defaultValue(kind, operator) }
    }
  }
  return { ...target, operator, value: condition.value }
}

const isPair = (value: unknown, item: (part: unknown) => boolean) => Array.isArray(value) && value.length === 2 && value.every(item)
const isIdList = (value: unknown) => Array.isArray(value) && value.length > 0 && value.every((item) => typeof item === 'string')

/** A custom field condition's value, by operator: the shapes the server takes for some field kind. */
function isCompleteCustomValue(operator: FilterOperator, value: unknown): boolean {
  switch (operator) {
    case 'contains':
      return typeof value === 'string' && value.trim().length > 0
    case 'less_than':
    case 'greater_than':
      return isFilterNumber(value)
    case 'is':
      return isFilterNumber(value) || typeof value === 'boolean' || isIdList(value)
    case 'between':
      return isPair(value, isFilterNumber) || isPair(value, isDateValue)
    case 'before':
    case 'after':
      return isDateValue(value)
    default:
      return isIdList(value)
  }
}

/** True when the server can evaluate it: list values present, text not blank, dates and numbers well-formed. */
export function isCompleteCondition(condition: Condition): boolean {
  if (condition.field === 'custom_field' && !condition.field_id) return false
  if (!operatorNeedsValue(condition.operator)) return true
  if (condition.field === 'custom_field') return isCompleteCustomValue(condition.operator, condition.value)
  const kind = FIELD_META[condition.field].kind
  if (kind === 'text') return typeof condition.value === 'string' && condition.value.trim().length > 0
  if (kind === 'date') {
    if (condition.operator !== 'between') return isDateValue(condition.value)
    return isPair(condition.value, isDateValue)
  }
  return listValue(condition).length > 0
}
