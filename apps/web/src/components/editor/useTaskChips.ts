import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useMemo } from 'react'
import { apiClient } from '../../api/client'
import { resolveTasks } from '../../api/generated/sdk.gen'
import { queryKeys } from '../../api/queryKeys'
import type { TaskStatusDef } from '../../features/tasks/api/models'
import type { ResolvedChip, TaskChipLookup } from './taskChipContext'

export type { ResolvedChip } from './taskChipContext'

/** What the batch resolve caches per identifier. Status is kept as an id so a
 *  status rename or recategorisation never needs a refetch. */
interface ChipTask {
  taskId: string
  title: string
  statusId: string
}

/**
 * Resolves every chip on a page in ONE `POST .../tasks/resolve`, then seeds a
 * per-identifier cache entry so a repeated chip — or the same chip while the
 * next batch is in flight — is free. The document only stores ids and a cached
 * identifier; the live title and status come from here, so renaming a task or a
 * project key updates every existing mention. The batch key lives under
 * `tasks.all`, so any task invalidation refreshes the chips too.
 */
export function useTaskChips(
  workspaceId: string,
  identifiers: string[],
  statuses: TaskStatusDef[],
): TaskChipLookup {
  const queryClient = useQueryClient()
  const unique = useMemo(() => Array.from(new Set(identifiers)), [identifiers])
  const key = useMemo(() => [...unique].sort(), [unique])

  const { data } = useQuery({
    queryKey: [...queryKeys.tasks.all(workspaceId), 'chips', key],
    enabled: unique.length > 0,
    staleTime: 30_000,
    queryFn: async () => {
      const { data } = await resolveTasks({
        client: apiClient,
        path: { workspace_id: workspaceId },
        body: { identifiers: unique },
        throwOnError: true,
      })
      const resolved = new Map<string, ChipTask>()
      for (const task of data?.items ?? []) {
        const chip: ChipTask = { taskId: task.id, title: task.title, statusId: task.status_id }
        resolved.set(task.identifier, chip)
        queryClient.setQueryData(queryKeys.taskChips(workspaceId, task.identifier), chip)
      }
      return resolved
    },
  })

  return useCallback(
    (identifier: string): ResolvedChip | undefined => {
      const task =
        data?.get(identifier) ??
        queryClient.getQueryData<ChipTask>(queryKeys.taskChips(workspaceId, identifier))
      if (!task) return undefined
      const status = statuses.find((candidate) => candidate.id === task.statusId)
      return {
        title: task.title,
        statusCategory: status?.category ?? 'unstarted',
        taskId: task.taskId,
        statusColor: status?.color,
      }
    },
    [data, queryClient, workspaceId, statuses],
  )
}
