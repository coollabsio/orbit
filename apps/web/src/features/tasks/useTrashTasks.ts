import { toast } from 'sonner'
import { confirmAction } from '@/components/common/confirmAction'
import type { Task } from '@/features/tasks/api/models'
import { useDeleteTask } from '@/features/tasks/api/tasks'

/** Moves tasks to trash after a confirmation. Resolves to false when the user cancels. */
export function useTrashTasks(workspaceId: string) {
  const deleteTask = useDeleteTask(workspaceId)
  return async (tasks: Task[]): Promise<boolean> => {
    const title = tasks.length === 1 ? `Move ${tasks[0].identifier} to trash?` : `Move ${tasks.length} tasks to trash?`
    if (!await confirmAction({ title, description: 'Sub-issues move to trash with their parent. You can restore them from trash later.', confirmLabel: 'Move to trash', danger: true })) return false
    const results = await Promise.allSettled(tasks.map((task) => deleteTask.mutateAsync({ taskId: task.id, version: task.version })))
    const failed = results.filter((result) => result.status === 'rejected').length
    if (failed > 0) toast.error(`Could not move ${failed} of ${tasks.length} tasks to trash. Try again.`)
    return true
  }
}
