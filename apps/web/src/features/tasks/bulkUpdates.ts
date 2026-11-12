import type { BulkItem, LabelRecord } from '@orbit/api/generated/types.gen'
import type { Cycle, Milestone, Task, TaskStatusDef } from '@/features/tasks/api/models'
import { labelKeyOf, labelRepresentatives, projectLabels } from '@/features/tasks/taskMeta'
import { resolveStatusId } from '@/features/tasks/tasksLib'

/** A status change by status key: each task takes the matching status of its own project; tasks already there are left out. */
export function statusUpdates(tasks: Task[], statuses: TaskStatusDef[], key: string | null): BulkItem[] {
  return tasks.flatMap((task) => {
    const statusId = resolveStatusId(statuses, task.projectId, key)
    return statusId && statusId !== task.statusId ? [{ id: task.id, expected_version: task.version, status_id: statusId }] : []
  })
}

export function priorityUpdates(tasks: Task[], priority: Task['priority']): BulkItem[] {
  return tasks.filter((task) => task.priority !== priority).map((task) => ({ id: task.id, expected_version: task.version, priority }))
}

/** Everyone has the assignee → remove from all; otherwise add to the tasks that miss it. */
export function assigneeToggleUpdates(tasks: Task[], userId: string): BulkItem[] {
  const everyone = tasks.every((task) => task.assigneeIds.includes(userId))
  return tasks.map((task) => ({
    id: task.id,
    expected_version: task.version,
    assignee_ids: everyone ? task.assigneeIds.filter((id) => id !== userId) : Array.from(new Set([...task.assigneeIds, userId])),
  }))
}

/** Adds the assignee to the tasks that miss it; never removes anyone. */
export function assignUpdates(tasks: Task[], userId: string): BulkItem[] {
  return tasks.filter((task) => !task.assigneeIds.includes(userId)).map((task) => ({ id: task.id, expected_version: task.version, assignee_ids: [...task.assigneeIds, userId] }))
}

/** One label per name among the labels of the tasks' projects, for a menu that labels them all at once. */
export function labelChoices(tasks: Task[], labels: LabelRecord[]): LabelRecord[] {
  const projectIds = new Set(tasks.map((task) => task.projectId))
  return labelRepresentatives(labels.filter((label) => projectIds.has(label.project_id))).labels
}

/** Each task with the label of its own project that has `label`'s name; tasks whose project has none are left out. */
function labelTargets(tasks: Task[], labels: LabelRecord[], label: LabelRecord) {
  const key = labelKeyOf(label)
  return tasks.flatMap((task) => {
    const own = projectLabels(labels, task.projectId).find((candidate) => labelKeyOf(candidate) === key)
    return own ? [{ task, labelId: own.id }] : []
  })
}

/** Every task that can take the label (by name) has it. */
export function everyTaskHasLabel(tasks: Task[], labels: LabelRecord[], label: LabelRecord): boolean {
  const targets = labelTargets(tasks, labels, label)
  return targets.length > 0 && targets.every(({ task, labelId }) => task.labels.includes(labelId))
}

/** Every task has the label (by name, in its own project) → remove from all; otherwise add to the tasks that miss it. */
export function labelToggleUpdates(tasks: Task[], labels: LabelRecord[], label: LabelRecord): BulkItem[] {
  const targets = labelTargets(tasks, labels, label)
  const everyone = everyTaskHasLabel(tasks, labels, label)
  return targets.map(({ task, labelId }) => ({
    id: task.id,
    expected_version: task.version,
    label_ids: everyone ? task.labels.filter((id) => id !== labelId) : Array.from(new Set([...task.labels, labelId])),
  }))
}

export function dueUpdates(tasks: Task[], start: string | null, end: string | null): BulkItem[] {
  return tasks
    .filter((task) => (task.dueStartAt ?? null) !== start || task.dueAt !== end)
    .map((task) => ({ id: task.id, expected_version: task.version, due_start_at: start, due_at: end }))
}

/** Milestones and cycles belong to one project: a pick reaches the tasks of that project; `null` clears it on every task. */
export function milestoneUpdates(tasks: Task[], milestone: Pick<Milestone, 'id' | 'project_id'> | null): BulkItem[] {
  return tasks
    .filter((task) => (milestone ? task.projectId === milestone.project_id && task.milestoneId !== milestone.id : Boolean(task.milestoneId)))
    .map((task) => ({ id: task.id, expected_version: task.version, milestone_id: milestone?.id ?? null }))
}

export function cycleUpdates(tasks: Task[], cycle: Pick<Cycle, 'id' | 'project_id'> | null): BulkItem[] {
  return tasks
    .filter((task) => (cycle ? task.projectId === cycle.project_id && task.cycleId !== cycle.id : Boolean(task.cycleId)))
    .map((task) => ({ id: task.id, expected_version: task.version, cycle_id: cycle?.id ?? null }))
}

export function estimateUpdates(tasks: Task[], estimate: number | null): BulkItem[] {
  return tasks.filter((task) => (task.estimate ?? null) !== estimate).map((task) => ({ id: task.id, expected_version: task.version, estimate }))
}

/** The one project of all `tasks`, whose milestones and cycles a menu can offer; null when they span projects. */
export function sharedProjectId(tasks: Task[]): string | null {
  const ids = new Set(tasks.map((task) => task.projectId))
  return ids.size === 1 ? [...ids][0]! : null
}
