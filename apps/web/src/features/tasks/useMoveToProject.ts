import { toast } from 'sonner'
import type { BulkItem } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { isTaskVersionConflict } from '@/features/tasks/api/conflicts'
import { taskIdentifier, type Project, type Task } from '@/features/tasks/api/models'
import { useBulkTasks } from '@/features/tasks/api/tasks'
import { keepIdentifiersTogether } from '@/lib/toast'

/** A move to another project. The server maps each task's status and gives it the project's next number; tasks already there are left out. */
export function projectUpdates(tasks: ReadonlyArray<Pick<Task, 'id' | 'version' | 'projectId'>>, projectId: string): BulkItem[] {
  return tasks.filter((task) => task.projectId !== projectId).map((task) => ({ id: task.id, expected_version: task.version, project_id: projectId }))
}

/** "Moved to OPS-3" for one task (its new identifier), "Moved 3 tasks to Operations" for several. */
export function moveToastMessage(project: Pick<Project, 'key' | 'name'>, moved: ReadonlyArray<{ id: string; number?: number }>): string {
  if (moved.length === 1) return `Moved to ${taskIdentifier(moved[0]!.id, project, moved[0]!.number)}`
  return `Moved ${moved.length} tasks to ${project.name}`
}

/** Moves tasks to another project in one bulk request (the context menu, the command menu and the detail sidebar). */
export function useMoveToProject(workspaceId: string) {
  const bulkTasks = useBulkTasks(workspaceId)
  return (tasks: ReadonlyArray<Pick<Task, 'id' | 'version' | 'projectId'>>, project: Project) => {
    const updates = projectUpdates(tasks, project.id)
    if (updates.length === 0) return
    bulkTasks.mutate(updates, {
      onSuccess: (page) => toast.success(keepIdentifiersTogether(moveToastMessage(project, page.items))),
      onError: (error) => {
        // a stale version asks to refresh (useBulkTasks); anything else (a GitHub-synced task) is explained here
        if (isTaskVersionConflict(error)) return
        toast.error(error instanceof ApiProblem ? error.detail : 'Could not move the task.')
      },
    })
  }
}
