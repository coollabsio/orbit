import type { QueryClient } from '@tanstack/react-query'
import type { PageTaskRecord, TaskRecord } from '../../../api/generated/types.gen'
import { queryKeys } from '../../../api/queryKeys'

export interface WorkspaceTaskSnapshot {
  entries: Array<[readonly unknown[], unknown]>
}

function mapTaskData(data: unknown, taskId: string, replace: (task: TaskRecord) => TaskRecord): unknown {
  if (!data || typeof data !== 'object') return data
  if ('id' in data && data.id === taskId && 'workspace_id' in data) return replace(data as TaskRecord)
  if ('items' in data && Array.isArray(data.items)) {
    return { ...data, items: data.items.map((item) => mapTaskData(item, taskId, replace)) }
  }
  if ('pages' in data && Array.isArray(data.pages)) {
    return { ...data, pages: data.pages.map((page) => mapTaskData(page, taskId, replace)) }
  }
  return data
}

export function patchWorkspaceTask(
  queryClient: QueryClient,
  workspaceId: string,
  taskId: string,
  patch: Partial<TaskRecord>,
): WorkspaceTaskSnapshot {
  const entries = queryClient.getQueriesData({ queryKey: queryKeys.tasks.all(workspaceId) })
  for (const [key, data] of entries) {
    queryClient.setQueryData(key, mapTaskData(data, taskId, (task) => ({ ...task, ...patch })))
  }
  return { entries }
}

export function restoreWorkspaceTasks(queryClient: QueryClient, snapshot: WorkspaceTaskSnapshot): void {
  for (const [key, data] of snapshot.entries) queryClient.setQueryData(key, data)
}

/**
 * Replaces the cached task with a mutation response. `duplicate_ids` and
 * `referenced_by` are detail-read-only (mutation responses always carry `[]`),
 * so the cached values are kept until the follow-up refetch replaces them;
 * otherwise every autosave would blink the Duplicates and Referenced by groups.
 */
export function reconcileWorkspaceTask(
  queryClient: QueryClient,
  workspaceId: string,
  record: TaskRecord,
): void {
  queryClient.setQueriesData(
    { queryKey: queryKeys.tasks.all(workspaceId) },
    (data) => mapTaskData(data, record.id, (cached) => ({
      ...record,
      duplicate_ids: cached.duplicate_ids ?? record.duplicate_ids,
      referenced_by: cached.referenced_by ?? record.referenced_by,
    })),
  )
}

export type { PageTaskRecord }
