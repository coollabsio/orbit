import type { BulkItem, LabelRecord } from '@/api/generated/types.gen'
import type { Cycle } from '@/features/tasks/api/cycles'
import type { Milestone } from '@/features/tasks/api/milestones'
import { addLabel } from '@/features/tasks/labelGroups'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
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

/** Every task has the label → remove from all; otherwise add to the tasks that miss it. */
/** `labels`: every label of the workspace, so that a label of a group replaces the task's other label of that group. */
export function labelToggleUpdates(tasks: Task[], labelId: string, labels: Array<Pick<LabelRecord, 'id' | 'name' | 'group_id' | 'group_name'>>): BulkItem[] {
  const everyone = tasks.every((task) => task.labels.includes(labelId))
  return tasks.map((task) => ({
    id: task.id,
    expected_version: task.version,
    label_ids: everyone ? task.labels.filter((id) => id !== labelId) : addLabel(task.labels, labelId, labels),
  }))
}

export function dueUpdates(tasks: Task[], start: string | null, end: string | null): BulkItem[] {
  return tasks
    .filter((task) => (task.dueStartAt ?? null) !== start || task.dueAt !== end)
    .map((task) => ({ id: task.id, expected_version: task.version, due_start_at: start, due_at: end }))
}

/** Sets or clears the milestone. The caller offers a milestone only when all the tasks are in its project. */
export function milestoneUpdates(tasks: Task[], milestoneId: string | null): BulkItem[] {
  return tasks
    .filter((task) => (task.milestoneId ?? null) !== milestoneId)
    .map((task) => ({ id: task.id, expected_version: task.version, milestone_id: milestoneId }))
}

/** The milestones that every task can take: those of the project when all the tasks are in one project, else none. */
export function sharedProjectMilestones(tasks: Task[], milestones: Milestone[]): Milestone[] {
  const [first] = tasks
  if (!first || tasks.some((task) => task.projectId !== first.projectId)) return []
  return milestones.filter((milestone) => milestone.project_id === first.projectId)
}

/** Sets or clears the cycle. The caller offers a cycle only when all the tasks are in its project. */
export function cycleUpdates(tasks: Task[], cycleId: string | null): BulkItem[] {
  return tasks
    .filter((task) => (task.cycleId ?? null) !== cycleId)
    .map((task) => ({ id: task.id, expected_version: task.version, cycle_id: cycleId }))
}

export function estimateUpdates(tasks: Task[], estimate: number | null): BulkItem[] {
  return tasks
    .filter((task) => (task.estimate ?? null) !== estimate)
    .map((task) => ({ id: task.id, expected_version: task.version, estimate }))
}

/** The project of the tasks when they are all in one project; a cycle and an estimate scale belong to one project. */
export function sharedProjectId(tasks: Task[]): string | null {
  const [first] = tasks
  return first && tasks.every((task) => task.projectId === first.projectId) ? first.projectId : null
}

/** The open cycles every task can take: those of the project when all the tasks are in one project, else none. */
export function sharedProjectCycles(tasks: Task[], cycles: Cycle[]): Cycle[] {
  const projectId = sharedProjectId(tasks)
  return projectId ? cycles.filter((cycle) => cycle.project_id === projectId && cycle.state !== 'completed') : []
}
