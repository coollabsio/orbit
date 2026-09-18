import { apiClient } from '../../../api/client'
import { getTask, resolveTasks } from '../../../api/generated/sdk.gen'

export interface TaskReference {
  id: string
  identifier: string
}

/** Resolves a typed identifier or a pasted task id to chip attrs; null leaves the text alone. */
export type TaskResolver = (input: { identifier?: string; taskId?: string }) => Promise<TaskReference | null>

/**
 * The default resolver behind "type `ORB-12 ` / paste a task link to get a
 * chip". Identifiers go through the same `POST .../tasks/resolve` the chips
 * use; a pasted `/tasks/<uuid>` link reads the task. Answers are memoised per
 * editor so retyping the same identifier is free.
 */
export function createTaskResolver(workspaceId: string): TaskResolver {
  const cache = new Map<string, Promise<TaskReference | null>>()
  const remember = (key: string, load: () => Promise<TaskReference | null>) => {
    const cached = cache.get(key)
    if (cached) return cached
    const pending = load().catch(() => {
      cache.delete(key)
      return null
    })
    cache.set(key, pending)
    return pending
  }

  return ({ identifier, taskId }) => {
    if (identifier) {
      return remember(`identifier:${identifier}`, async () => {
        const { data } = await resolveTasks({
          client: apiClient,
          path: { workspace_id: workspaceId },
          body: { identifiers: [identifier] },
          throwOnError: true,
        })
        const task = data?.items.find((item) => item.identifier === identifier)
        return task ? { id: task.id, identifier: task.identifier } : null
      })
    }
    if (taskId) {
      return remember(`id:${taskId}`, async () => {
        const { data } = await getTask({
          client: apiClient,
          path: { workspace_id: workspaceId, task_id: taskId },
          throwOnError: true,
        })
        return data ? { id: data.id, identifier: data.identifier } : null
      })
    }
    return Promise.resolve(null)
  }
}
