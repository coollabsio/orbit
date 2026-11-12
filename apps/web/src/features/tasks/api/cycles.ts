import { queryOptions, useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { createCycle, deleteCycle, listCycles, moveUnfinishedCycleTasks, updateCycle } from '@orbit/api/generated/sdk.gen'
import type { TaskCycleBody, TaskCycleUpdateBody } from '@orbit/api/generated/types.gen'
import type { Cycle } from './models'

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

function cyclesQuery(workspaceId: string, projectId: string) {
  return queryOptions({
    queryKey: queryKeys.cycles(workspaceId, projectId),
    enabled: Boolean(projectId),
    queryFn: async (): Promise<Cycle[]> => {
      // one page: a project's list is not paginated
      const { data } = await listCycles({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, throwOnError: true })
      return required(data, 'Cycles response was empty.').items.sort((a, b) => a.starts_at.localeCompare(b.starts_at))
    },
  })
}

/** One project's cycles by start date. */
export function useCycles(workspaceId: string, projectId: string | undefined) {
  return useQuery(cyclesQuery(workspaceId, projectId ?? ''))
}

/** The cycles of several projects (the projects of the tasks in view), for chips and pickers. */
export function useProjectsCycles(workspaceId: string, projectIds: readonly string[]): Cycle[] {
  const results = useQueries({ queries: [...new Set(projectIds)].map((projectId) => cyclesQuery(workspaceId, projectId)) })
  return results.flatMap((result) => result.data ?? [])
}

export function useCreateCycle(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: TaskCycleBody) => {
      const { data } = await createCycle({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, body, throwOnError: true })
      return required(data, 'Create cycle response was empty.')
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.cycles(workspaceId, projectId) }),
  })
}

export function useUpdateCycle(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ cycleId, body }: { cycleId: string; body: TaskCycleUpdateBody }) => {
      const { data } = await updateCycle({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId, cycle_id: cycleId }, body, throwOnError: true })
      return required(data, 'Update cycle response was empty.')
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.cycles(workspaceId, projectId) }),
  })
}

/** Its tasks keep living without a cycle. */
export function useDeleteCycle(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (cycle: Pick<Cycle, 'id' | 'version'>) => {
      await deleteCycle({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId, cycle_id: cycle.id }, query: { expected_version: cycle.version }, throwOnError: true })
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.cycles(workspaceId, projectId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
    },
  })
}

/** Moves the cycle's tasks that are not done or cancelled to the next cycle by date; 409 `no_next_cycle` without one. */
export function useMoveUnfinishedCycleTasks(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (cycleId: string) => {
      const { data } = await moveUnfinishedCycleTasks({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId, cycle_id: cycleId }, throwOnError: true })
      return required(data, 'Move unfinished tasks response was empty.')
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.cycles(workspaceId, projectId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
    },
  })
}
