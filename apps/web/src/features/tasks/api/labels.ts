import { useQuery } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '../../../api/client'
import { listLabels } from '../../../api/generated/sdk.gen'
import { queryKeys } from '../../../api/queryKeys'

type ApiClient = ReturnType<typeof createApiClient>

export function labelsQueryOptions(workspaceId: string, client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.labels(workspaceId),
    queryFn: async () => {
      const labels = []
      let cursor: string | undefined
      do {
        const { data } = await listLabels({
          client,
          path: { workspace_id: workspaceId },
          query: { limit: 100, cursor },
          throwOnError: true,
        })
        if (!data) throw new Error('Labels response was empty.')
        labels.push(...data.items)
        cursor = data.next_cursor ?? undefined
      } while (cursor)
      return labels
    },
  }
}

export function useLabels(workspaceId: string) {
  return useQuery(labelsQueryOptions(workspaceId))
}
