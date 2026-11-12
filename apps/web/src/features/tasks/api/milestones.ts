import { queryOptions, useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { createMilestone, deleteMilestone, listMilestones, reorderMilestones, updateMilestone } from '@orbit/api/generated/sdk.gen'
import type { ReorderItem, TaskMilestoneBody, TaskMilestoneUpdateBody } from '@orbit/api/generated/types.gen'
import type { Milestone } from './models'

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

function milestonesQuery(workspaceId: string, projectId: string) {
  return queryOptions({
    queryKey: queryKeys.milestones(workspaceId, projectId),
    enabled: Boolean(projectId),
    queryFn: async (): Promise<Milestone[]> => {
      // one page: a project's list is not paginated
      const { data } = await listMilestones({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, throwOnError: true })
      return required(data, 'Milestones response was empty.').items.sort((a, b) => a.position - b.position)
    },
  })
}

/** One project's milestones in their order. */
export function useMilestones(workspaceId: string, projectId: string | undefined) {
  return useQuery(milestonesQuery(workspaceId, projectId ?? ''))
}

/** The milestones of several projects (the projects of the tasks in view), for chips and pickers. */
export function useProjectsMilestones(workspaceId: string, projectIds: readonly string[]): Milestone[] {
  const results = useQueries({ queries: [...new Set(projectIds)].map((projectId) => milestonesQuery(workspaceId, projectId)) })
  return results.flatMap((result) => result.data ?? [])
}

export function useCreateMilestone(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: TaskMilestoneBody) => {
      const { data } = await createMilestone({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, body, throwOnError: true })
      return required(data, 'Create milestone response was empty.')
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.milestones(workspaceId, projectId) }),
  })
}

export function useUpdateMilestone(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ milestoneId, body }: { milestoneId: string; body: TaskMilestoneUpdateBody }) => {
      const { data } = await updateMilestone({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId, milestone_id: milestoneId }, body, throwOnError: true })
      return required(data, 'Update milestone response was empty.')
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.milestones(workspaceId, projectId) }),
  })
}

/** Its tasks keep living without a milestone. */
export function useDeleteMilestone(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (milestone: Pick<Milestone, 'id' | 'version'>) => {
      await deleteMilestone({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId, milestone_id: milestone.id }, query: { expected_version: milestone.version }, throwOnError: true })
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.milestones(workspaceId, projectId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
    },
  })
}

export function useReorderMilestones(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (items: ReorderItem[]) => {
      const { data } = await reorderMilestones({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, body: { items }, throwOnError: true })
      return required(data, 'Reorder milestones response was empty.')
    },
    // the new order shows at once; the response or a refetch settles it
    onMutate: (items) => {
      const key = queryKeys.milestones(workspaceId, projectId)
      const previous = queryClient.getQueryData<Milestone[]>(key)
      const position = new Map(items.map((item) => [item.id, item.position]))
      queryClient.setQueryData<Milestone[]>(key, (current) => current
        ?.map((milestone) => ({ ...milestone, position: position.get(milestone.id) ?? milestone.position }))
        .sort((a, b) => a.position - b.position))
      return { previous }
    },
    onError: (_error, _items, context) => {
      if (context?.previous) queryClient.setQueryData(queryKeys.milestones(workspaceId, projectId), context.previous)
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.milestones(workspaceId, projectId) }),
  })
}
