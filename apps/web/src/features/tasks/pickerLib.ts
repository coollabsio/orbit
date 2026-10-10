import type { TaskStatusDef } from '@/features/tasks/api/models'
import { projectStatuses } from '@/features/tasks/taskMeta'

/** The statuses a task can move to in the status picker: its project's, in order. Duplicate needs a canonical task,
 *  so it is offered only where the caller can ask for one. Triage is where tasks come in, not a place to move a
 *  task to: it shows only while the task is in it (`currentStatusId`). */
export function statusPickerOptions(statuses: TaskStatusDef[], projectId: string, withDuplicate: boolean, currentStatusId?: string): TaskStatusDef[] {
  return projectStatuses(statuses, projectId).filter((status) =>
    (withDuplicate || status.category !== 'duplicate') && (status.category !== 'triage' || status.id === currentStatusId))
}

/** The assignee list after checking or unchecking `userId` in the assignee picker. */
export function toggleAssignee(assigneeIds: string[], userId: string): string[] {
  return assigneeIds.includes(userId) ? assigneeIds.filter((id) => id !== userId) : [...assigneeIds, userId]
}
