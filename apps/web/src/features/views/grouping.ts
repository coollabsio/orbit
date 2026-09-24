import type { CreateTaskBody, LabelRecord, ProjectRecord, TaskUpdateBody } from '@/api/generated/types.gen'
import type { Task, TaskPriority, TaskStatusDef } from '@/features/tasks/api/models'
import { PRIORITY_LABEL, PRIORITY_ORDER, statusKeyOf } from '@/features/tasks/taskMeta'
import { statusGroups } from '@/features/tasks/tasksLib'
import type { User } from '@/features/workspaces/models'
import { isGroup, resolveDateValue, type Condition, type DateValue, type FilterGroup, type GroupBy } from '@/features/views/viewState'

export type TaskGroup = { key: string; field: GroupBy; value: string | null; label: string; tasks: Task[] }
/**
 * `statuses` and `projects` should already be scoped to the page (a project page passes only its project's
 * statuses), so empty groups never list another project's workflow.
 */
export type GroupContext = { statuses: TaskStatusDef[]; members: User[]; labels: LabelRecord[]; projects: ProjectRecord[]; currentUserId: string; showEmpty: boolean }
export type TaskCreateDefaults = Partial<CreateTaskBody>
export type DropUpdate = { taskId: string; patch: Partial<TaskUpdateBody> } | { error: string } | null

type Bucket = { value: string | null; label: string }

const byName = (a: { name: string }, b: { name: string }) => a.name.localeCompare(b.name)

function bucketsFor(groupBy: GroupBy, ctx: GroupContext): Bucket[] {
  switch (groupBy) {
    case 'status':
      return statusGroups(ctx.statuses, null).map((group): Bucket => ({ value: group.key, label: group.name }))
    case 'priority':
      return PRIORITY_ORDER.map((priority): Bucket => ({ value: priority, label: PRIORITY_LABEL[priority] }))
    case 'assignee': {
      const me = ctx.members.filter((member) => member.id === ctx.currentUserId)
      const others = ctx.members.filter((member) => member.id !== ctx.currentUserId).sort(byName)
      return [...[...me, ...others].map((member): Bucket => ({ value: member.id, label: member.name })), { value: null, label: 'No assignee' }]
    }
    case 'label':
      return [...[...ctx.labels].sort(byName).map((label): Bucket => ({ value: label.id, label: label.name })), { value: null, label: 'No label' }]
    case 'project':
      return [...ctx.projects].sort(byName).map((project): Bucket => ({ value: project.id, label: project.name }))
    case 'none':
      return [{ value: null, label: 'All tasks' }]
  }
}

/** Every group a task belongs to; unknown or inactive ids fall into the "No …" group. */
function valuesOf(task: Task, groupBy: GroupBy, statusKeys: Map<string, string>, known: Set<string>): Array<string | null> {
  const present = (ids: string[]) => {
    const kept = [...new Set(ids)].filter((id) => known.has(id))
    return kept.length > 0 ? kept : [null]
  }
  switch (groupBy) {
    case 'status': {
      const key = statusKeys.get(task.statusId)
      return key ? [key] : []
    }
    case 'priority':
      return [task.priority]
    case 'assignee':
      return present(task.assigneeIds)
    case 'label':
      return present(task.labels)
    case 'project':
      return [task.projectId]
    case 'none':
      return [null]
  }
}

/**
 * Ordered groups for a grouping field (spec §4). Tasks keep their input (server) order inside a group;
 * a task with several assignees or labels appears in each of their groups.
 */
export function groupTasks(tasks: Task[], groupBy: GroupBy, ctx: GroupContext): TaskGroup[] {
  const buckets = bucketsFor(groupBy, ctx)
  const known = new Set(buckets.flatMap((bucket) => (bucket.value === null ? [] : [bucket.value])))
  const statusKeys = new Map(ctx.statuses.map((status) => [status.id, statusKeyOf(status)]))
  const members = new Map<string | null, Task[]>(buckets.map((bucket) => [bucket.value, []]))
  for (const task of tasks) {
    for (const value of valuesOf(task, groupBy, statusKeys, known)) members.get(value)?.push(task)
  }
  return buckets
    .map((bucket): TaskGroup => ({
      key: groupBy === 'none' ? 'all' : `${groupBy}:${bucket.value ?? 'none'}`,
      field: groupBy,
      value: bucket.value,
      label: bucket.label,
      tasks: members.get(bucket.value) ?? [],
    }))
    .filter((group) => groupBy === 'none' || ctx.showEmpty || group.tasks.length > 0)
}

function swapValue(current: string[], from: string | null, to: string | null): string[] | null {
  const next = current.filter((id) => id !== from)
  if (to !== null && !next.includes(to)) next.push(to)
  return next.length === current.length && next.every((id, index) => id === current[index]) ? null : next
}

/** The task update for dragging `task` from group value `from` to `to` (spec §4 "Drag across groups"). */
export function dropUpdate(task: Task, field: GroupBy, from: string | null, to: string | null, ctx: GroupContext): DropUpdate {
  if (from === to) return null
  const update = (patch: Partial<TaskUpdateBody>): DropUpdate => ({ taskId: task.id, patch: { expected_version: task.version, ...patch } })
  switch (field) {
    case 'status': {
      if (to === null) return null
      const target = ctx.statuses.find((status) => status.projectId === task.projectId && statusKeyOf(status) === to)
      if (!target) return { error: `No matching status in ${ctx.projects.find((project) => project.id === task.projectId)?.name ?? 'this project'}` }
      return target.id === task.statusId ? null : update({ status_id: target.id })
    }
    case 'priority':
      return to === null || to === task.priority ? null : update({ priority: to })
    case 'assignee': {
      const next = swapValue(task.assigneeIds, from, to)
      return next ? update({ assignee_ids: next }) : null
    }
    case 'label': {
      const next = swapValue(task.labels, from, to)
      return next ? update({ label_ids: next }) : null
    }
    case 'project': // no API moves a task between projects: dragging is disabled
    case 'none':
      return null
  }
}

function isDateValue(value: unknown): value is DateValue {
  if (!value || typeof value !== 'object') return false
  const date = value as { absolute?: unknown; relative?: unknown }
  return typeof date.absolute === 'string' || typeof date.relative === 'string'
}

/** Due dates that keep a new task inside a date-filtered page (Overdue, Due soon, This week, My week). */
function dueDefaults(condition: Condition, now: Date): TaskCreateDefaults {
  if (condition.operator === 'before' && isDateValue(condition.value)) {
    return { due_at: new Date(Date.parse(`${resolveDateValue(condition.value, now)}T00:00:00.000Z`) - 1).toISOString() }
  }
  const range = condition.value
  if (condition.operator === 'between' && Array.isArray(range) && range.length === 2 && isDateValue(range[0]) && isDateValue(range[1])) {
    const [start, end] = range as [DateValue, DateValue]
    const first = resolveDateValue(start, now)
    const wholeWeek = 'relative' in start && start.relative === 'start_of_week' && 'relative' in end && end.relative === 'end_of_week'
    return wholeWeek
      ? { due_start_at: `${first}T00:00:00.000Z`, due_at: `${resolveDateValue(end, now)}T12:00:00.000Z` }
      : { due_at: `${first}T12:00:00.000Z` }
  }
  return {}
}

/**
 * Defaults for a task created on a filtered page (spec §3 "New-task defaults"): root-AND `is` conditions with
 * exactly one value (label: `includes_any` with one value, or every `includes_all` value), `me` → the current user,
 * the status key → that status in the target project, plus due dates for date presets.
 * Unknown ids are ignored. `project_id` falls back to `ctx.targetProjectId`.
 */
export function createDefaultsFromFilter(
  filter: FilterGroup,
  ctx: GroupContext & { targetProjectId: string | null },
  now: Date = new Date(),
): TaskCreateDefaults {
  const conditions = filter.op === 'and' ? filter.children.filter((child): child is Condition => !isGroup(child)) : []
  const single = (condition: Condition) =>
    Array.isArray(condition.value) && condition.value.length === 1 && typeof condition.value[0] === 'string' ? condition.value[0] : null
  const defaults: TaskCreateDefaults = {}
  let projectId = ctx.targetProjectId
  let statusKey: string | null = null
  for (const condition of conditions) {
    const is = condition.operator === 'is' ? single(condition) : null
    switch (condition.field) {
      case 'project':
        if (is && ctx.projects.some((project) => project.id === is)) projectId = is
        break
      case 'assignee': {
        const userId = is === 'me' ? ctx.currentUserId : is
        if (userId && ctx.members.some((member) => member.id === userId)) defaults.assignee_ids = [userId]
        break
      }
      case 'label': {
        const values = condition.operator === 'includes_all' && Array.isArray(condition.value)
          ? condition.value
          : condition.operator === 'includes_any' ? [single(condition)] : []
        const ids = values.filter((id): id is string => typeof id === 'string' && ctx.labels.some((label) => label.id === id))
        if (ids.length > 0) defaults.label_ids = [...new Set([...(defaults.label_ids ?? []), ...ids])]
        break
      }
      case 'priority':
        if (is && PRIORITY_ORDER.includes(is as TaskPriority)) defaults.priority = is
        break
      case 'status':
        if (is) statusKey = is
        break
      case 'due_date':
        Object.assign(defaults, dueDefaults(condition, now))
        break
      default:
        break
    }
  }
  if (projectId) {
    defaults.project_id = projectId
    const status = statusKey ? ctx.statuses.find((item) => item.projectId === projectId && statusKeyOf(item) === statusKey) : undefined
    if (status) defaults.status_id = status.id
  }
  return defaults
}
