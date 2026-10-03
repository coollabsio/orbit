import { toast } from 'sonner'
import type { TaskUpdateBody } from '@/api/generated/types.gen'
import { isTaskVersionConflict } from '@/features/tasks/api/conflicts'
import { useUpdateTask } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

export type TaskChanges = Omit<TaskUpdateBody, 'expected_version'>

type UpdateTask = Pick<ReturnType<typeof useUpdateTask>, 'mutate' | 'mutateAsync'>

/**
 * One entry per task with a save in flight, shared by every picker of that task (a row's status and priority
 * pickers, the same task in a list row and a relation row); removed when the task has nothing left to save.
 * `next` collects the changes that wait; `failure` and `updateTask` are the latest caller's.
 */
const queues = new Map<string, { next: TaskChanges | null; failure: string; updateTask: UpdateTask }>()

function send(key: string, taskId: string, version: number, changes: TaskChanges) {
  const entry = queues.get(key)
  if (!entry) return
  const { updateTask } = entry
  const variables = { taskId, body: { ...changes, expected_version: version } }
  // mutateAsync: per-call `mutate` callbacks are dropped when the calling picker unmounts, which would strand the queue
  updateTask.mutateAsync(variables).then(
    (record) => {
      const next = entry.next
      entry.next = null
      if (next) send(key, taskId, record.version, next)
      else queues.delete(key)
    },
    (error: Error) => {
      queues.delete(key)
      if (!isTaskVersionConflict(error)) toast.error(entry.failure, { action: { label: 'Retry', onClick: () => updateTask.mutate(variables) } })
    },
  )
}

/**
 * Saves a task from an in-place picker (list rows, board cards, timeline labels, sub-issue and relation rows) through
 * `useUpdateTask`. A change made while a save of the same task is in flight, from this picker or any other, waits
 * for it and goes out with the version that save returned, so quick successive picks (a day, then the range's last
 * day; a status, then a priority) do not conflict with each other. Waiting changes merge into one save: fields add
 * up and a later value for the same field wins. A failed save drops the waiting changes and shows a toast with Retry
 * (a version conflict already asks to refresh). `task` may be undefined while a page loads; changes are then ignored.
 */
export function useTaskPickerUpdate(task: { id: string; version: number } | undefined, failure: string) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  return (changes: TaskChanges) => {
    if (!task) return
    const key = `${workspace.id}:${task.id}`
    const entry = queues.get(key)
    if (entry) {
      Object.assign(entry, { next: { ...entry.next, ...changes }, failure, updateTask })
      return
    }
    queues.set(key, { next: null, failure, updateTask })
    send(key, task.id, task.version, changes)
  }
}
