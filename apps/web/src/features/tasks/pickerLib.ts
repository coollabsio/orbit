import type { TaskStatusDef } from '@/features/tasks/api/models'
import { projectStatuses } from '@/features/tasks/taskMeta'

/** The statuses a task can move to in the status picker: its project's, in order. Duplicate needs a canonical task,
 *  so it is offered only where the caller can ask for one. */
export function statusPickerOptions(statuses: TaskStatusDef[], projectId: string, withDuplicate: boolean): TaskStatusDef[] {
  const own = projectStatuses(statuses, projectId)
  return withDuplicate ? own : own.filter((status) => status.category !== 'duplicate')
}

/** The assignee list after checking or unchecking `userId` in the assignee picker. */
export function toggleAssignee(assigneeIds: string[], userId: string): string[] {
  return assigneeIds.includes(userId) ? assigneeIds.filter((id) => id !== userId) : [...assigneeIds, userId]
}
