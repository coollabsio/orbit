import { toast } from 'sonner'
import { isTaskVersionConflict } from '@/features/tasks/api/conflicts'
import { BulkTaskLimitError, MAX_BULK_TASK_UPDATES } from '@/features/tasks/api/tasks'

/**
 * The one error path for drag-and-drop moves (list and board): a toast, except for version conflicts,
 * which the task mutations already answer with "Refresh task?" (and a refetch). Pass it as `onError`.
 */
export function reportMoveError(error: Error) {
  if (isTaskVersionConflict(error)) return
  toast.error(error instanceof BulkTaskLimitError
    ? `This move would update ${error.count} tasks. Move it in smaller steps so each drop affects at most ${MAX_BULK_TASK_UPDATES} tasks.`
    : 'Could not move the task.')
}
