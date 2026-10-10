import { useQuery } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { getInsightOpen, getInsightThroughput, getMilestoneBurnup } from '@/api/generated/sdk.gen'
import { queryKeys } from '@/api/queryKeys'

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

/** Week limits use the timezone of the viewer. */
const viewerTimezone = () => Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'

export type OpenDimension = 'status' | 'assignee' | 'priority' | 'label'

export function useThroughput(workspaceId: string, projectId: string, weeks = 12) {
  return useQuery({
    queryKey: [...queryKeys.insights(workspaceId, projectId), 'throughput', weeks],
    queryFn: async () => {
      const { data } = await getInsightThroughput({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, query: { tz: viewerTimezone(), weeks }, throwOnError: true })
      return required(data, 'Throughput response was empty.')
    },
  })
}

export function useOpenBy(workspaceId: string, projectId: string, by: OpenDimension) {
  return useQuery({
    queryKey: [...queryKeys.insights(workspaceId, projectId), 'open', by],
    queryFn: async () => {
      const { data } = await getInsightOpen({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, query: { by }, throwOnError: true })
      return required(data, 'Open tasks response was empty.')
    },
  })
}

export function useMilestoneBurnup(workspaceId: string, projectId: string, milestoneId: string) {
  return useQuery({
    queryKey: [...queryKeys.insights(workspaceId, projectId), 'burnup', milestoneId],
    queryFn: async () => {
      const { data } = await getMilestoneBurnup({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId, milestone_id: milestoneId }, query: { tz: viewerTimezone() }, throwOnError: true })
      return required(data, 'Burn-up response was empty.')
    },
  })
}
