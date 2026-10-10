import { toast } from 'sonner'
import { ApiProblem } from '@/api/problem'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import { useArchiveTasks } from '@/features/tasks/api/tasks'
import { isClosedCategory } from '@/features/tasks/taskMeta'

/** True when each task is closed: only closed tasks (with their closed trees) can go to the archive. */
export function canArchive(tasks: Task[], statuses: TaskStatusDef[]): boolean {
  return tasks.length > 0 && tasks.every((task) => !task.archivedAt && isClosedCategory(statuses.find((status) => status.id === task.statusId)?.category))
}

/** Archive and restore, with a toast for the result. The server moves each task with its tree. */
export function useArchiveActions(workspaceId: string) {
  const mutation = useArchiveTasks(workspaceId)
  const run = (tasks: Array<Pick<Task, 'id'>>, archive: boolean) =>
    mutation.mutateAsync({ taskIds: tasks.map((task) => task.id), archive }).then(
      (count) => {
        toast(archive ? `Archived ${count === 1 ? '1 task' : `${count} tasks`}` : `Restored ${count === 1 ? '1 task' : `${count} tasks`}`)
        return true
      },
      (error: unknown) => {
        toast.error(error instanceof ApiProblem && error.code === 'archive_open_tree'
          ? 'Not archived: a parent or a sub-issue of a selected task is still open.'
          : archive ? 'Could not archive the tasks.' : 'Could not restore the tasks.')
        return false
      },
    )
  return { archive: (tasks: Array<Pick<Task, 'id'>>) => run(tasks, true), restore: (tasks: Array<Pick<Task, 'id'>>) => run(tasks, false), pending: mutation.isPending }
}
