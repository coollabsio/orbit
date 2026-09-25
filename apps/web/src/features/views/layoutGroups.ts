import type { BulkItem, CreateTaskBody, TaskUpdateBody } from '@/api/generated/types.gen'
import type { Task } from '@/features/tasks/api/models'
import { dropUpdate, groupTasks, type GroupContext, type TaskGroup } from './grouping'
import type { DisplayOptions, GroupBy } from './viewState'

/** Layout glue between display options, `groupTasks` and the task mutations. Pure: no React, no fetch. */

export type GroupField = Exclude<GroupBy, 'none'>
type GroupValue = { field: GroupField; value: string | null }
/** Outer group first, then the sub-group (list) or swim lane (board). */
export type GroupValues = GroupValue[]
export type TaskPatch = Omit<TaskUpdateBody, 'expected_version'>

/** Key of the single implicit group when `group_by` is 'none'. */
export const ALL_TASKS_KEY = 'all'

export function valuesOf(...groups: Array<Pick<TaskGroup, 'field' | 'value'>>): GroupValues {
  const values: GroupValues = []
  for (const group of groups) if (group.field !== 'none') values.push({ field: group.field, value: group.value })
  return values
}

/** Stable id of a drop zone / collapsible group: `status=unstarted:todo/priority=urgent`. */
export function zoneIdOf(values: GroupValues): string {
  if (values.length === 0) return ALL_TASKS_KEY
  return values.map(({ field, value }) => `${field}=${value ?? ''}`).join('/')
}

function valueIn(values: GroupValues, field: GroupField): string | null {
  return values.find((item) => item.field === field)?.value ?? null
}

/** The sub-group field that applies: none without a group, and never the group field itself. */
function subGroupOf(display: DisplayOptions): GroupField | null {
  const { group_by: group, sub_group_by: sub } = display
  return group !== 'none' && sub !== 'none' && sub !== group ? sub : null
}

export type ListSection = { group: TaskGroup; subGroups: TaskGroup[] | null }

/**
 * List sections in display order. `group_by: 'none'` is one headerless section. Sub-groups are built from
 * each group's own tasks; `show_empty_groups` applies to groups and sub-groups alike (spec §4).
 */
export function listSections(tasks: Task[], display: DisplayOptions, ctx: GroupContext): ListSection[] {
  if (display.group_by === 'none') {
    return [{ group: { key: ALL_TASKS_KEY, field: 'none', value: null, label: 'All tasks', tasks }, subGroups: null }]
  }
  const sub = subGroupOf(display)
  const shown = { ...ctx, showEmpty: display.show_empty_groups }
  return groupTasks(tasks, display.group_by, shown).map((group) => ({
    group,
    subGroups: sub ? groupTasks(group.tasks, sub, shown) : null,
  }))
}

/** Group fields a drop may rewrite. Project is excluded: a drop never moves a task to another project (spec §4). */
function movableFields(display: DisplayOptions): GroupField[] {
  const fields: GroupField[] = []
  for (const field of [display.group_by, subGroupOf(display)]) {
    if (field && field !== 'none' && field !== 'project') fields.push(field)
  }
  return fields
}

/** Rows/cards are drag sources only when some drop could change something. */
export function canDrag(display: DisplayOptions): boolean {
  return display.order_by === 'manual' || movableFields(display).length > 0
}

/** A zone accepts a drop unless it would change the task's project. */
export function acceptsDrop(from: GroupValues, to: GroupValues): boolean {
  return to.every((target) => target.field !== 'project' || valueIn(from, 'project') === target.value)
}

export type DropPlan =
  | { kind: 'none' }
  | { kind: 'update'; patch: TaskPatch }
  | { kind: 'duplicate' }
  | { kind: 'error'; message: string }

/**
 * What a drop from one zone to another means for `task`. Every changed field goes through `dropUpdate`;
 * the patches merge into one write. One refusal refuses the whole drop (nothing is written): a project
 * change (the layouts never offer such a target, this is the guard), or a status the task's project lacks.
 * Entering the Duplicate status needs a canonical task, so it wins over any other change and only opens the picker.
 */
export function planDrop(task: Task, from: GroupValues, to: GroupValues, ctx: GroupContext): DropPlan {
  if (!acceptsDrop(from, to)) return { kind: 'error', message: 'Tasks can’t be moved between projects.' }
  const changed = to.filter((target) => valueIn(from, target.field) !== target.value)
  if (changed.some((target) => target.field === 'status' && target.value?.startsWith('duplicate:'))) return { kind: 'duplicate' }
  let patch: TaskPatch = {}
  for (const target of changed) {
    const result = dropUpdate(task, target.field, valueIn(from, target.field), target.value, ctx)
    if (result === null) continue
    if ('error' in result) return { kind: 'error', message: result.error }
    const { expected_version: _version, ...fields } = result.patch
    patch = { ...patch, ...fields }
  }
  return Object.keys(patch).length > 0 ? { kind: 'update', patch } : { kind: 'none' }
}

/**
 * Bulk items that put `task` at `index` among `destination` (the zone's tasks in manual order, the task
 * itself included when it is already there) and apply `patch` to it. Only slots whose task or position
 * changes are written, so a drop into a long group stays within the bulk limit when it can.
 */
export function placementUpdates(task: Task, destination: Task[], index: number, patch: TaskPatch = {}): BulkItem[] {
  const current = [...destination]
  const currentIndex = current.findIndex((item) => item.id === task.id)
  const ordered = current.filter((item) => item.id !== task.id)
  ordered.splice(Math.max(0, Math.min(index, ordered.length)), 0, task)
  const existingPositions = current.map((item) => item.position)
  const positions = currentIndex === -1
    ? [...existingPositions, (existingPositions.at(-1) ?? -1) + 1]
    : existingPositions
  const positionsAreOrdered = positions.every((position, itemIndex) =>
    Number.isInteger(position) && (itemIndex === 0 || position > positions[itemIndex - 1]!))
  const slots = positionsAreOrdered ? positions : ordered.map((_, itemIndex) => itemIndex)
  const hasPatch = Object.keys(patch).length > 0

  return ordered.flatMap((item, itemIndex) => {
    const position = slots[itemIndex]!
    const moving = item.id === task.id
    if (current[itemIndex]?.id === item.id && item.position === position && !(moving && hasPatch)) return []
    return [{ id: item.id, expected_version: item.version, position, ...(moving ? patch : {}) }]
  })
}

export type GroupCreateFields = {
  projectId: string | null
  /** Status key (`category:name`), resolved against the target project by the caller. */
  statusKey: string | null
  body: Pick<CreateTaskBody, 'priority' | 'assignee_ids' | 'label_ids'>
}

/** Fields for a task created from a group header; each wins over the filter default for its field. */
export function groupCreateFields(values: GroupValues): GroupCreateFields {
  const result: GroupCreateFields = { projectId: null, statusKey: null, body: {} }
  for (const { field, value } of values) {
    if (field === 'project') result.projectId = value
    else if (field === 'status') result.statusKey = value
    else if (field === 'priority') result.body.priority = value ?? 'none'
    else if (field === 'assignee') result.body.assignee_ids = value ? [value] : []
    else if (field === 'label') result.body.label_ids = value ? [value] : []
  }
  return result
}
