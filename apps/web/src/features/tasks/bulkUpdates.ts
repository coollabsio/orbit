import type { BulkItem } from '@/api/generated/types.gen'
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
export function labelToggleUpdates(tasks: Task[], labelId: string): BulkItem[] {
  const everyone = tasks.every((task) => task.labels.includes(labelId))
  return tasks.map((task) => ({
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
