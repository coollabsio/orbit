import { useRef } from 'react'
import { toast } from 'sonner'
import type { TaskUpdateBody } from '@/api/generated/types.gen'
import { isTaskVersionConflict } from '@/features/tasks/api/conflicts'
import { useUpdateTask } from '@/features/tasks/api/tasks'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

export type TaskChanges = Omit<TaskUpdateBody, 'expected_version'>

/**
 * Saves a task from an in-place picker (list rows, board cards, timeline labels, sub-issue and relation rows) through
 * `useUpdateTask`. A change made while the previous one is still saving waits for it and goes out with the version
 * that save returned, so quick successive picks (a day, then the range's last day) do not conflict with each other;
 * only the latest waiting change is kept. A failed save shows a toast with Retry (a version conflict already asks to
 * refresh). `task` may be undefined while a page loads; changes are then ignored.
 */
export function useTaskPickerUpdate(task: { id: string; version: number } | undefined, failure: string) {
  const { workspace } = useWorkspace()
  const updateTask = useUpdateTask(workspace.id)
  const queue = useRef<{ saving: boolean; next: TaskChanges | null }>({ saving: false, next: null })
  const send = (taskId: string, version: number, changes: TaskChanges) => {
    queue.current.saving = true
    updateTask.mutate({ taskId, body: { ...changes, expected_version: version } }, {
      onSuccess: (record) => {
        const next = queue.current.next
        queue.current = { saving: false, next: null }
        if (next) send(taskId, record.version, next)
      },
      onError: (error, variables) => {
        queue.current = { saving: false, next: null }
        if (!isTaskVersionConflict(error)) toast.error(failure, { action: { label: 'Retry', onClick: () => updateTask.mutate(variables) } })
      },
    })
  }
  return (changes: TaskChanges) => {
    if (!task) return
    if (queue.current.saving) queue.current.next = changes
    else send(task.id, task.version, changes)
  }
}
