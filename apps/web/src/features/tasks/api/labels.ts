import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { createLabel, listLabels } from '@/api/generated/sdk.gen'
import type { LabelBody, LabelRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'

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

export function useCreateLabel(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: LabelBody) => {
      const { data } = await createLabel({
        client: apiClient,
        path: { workspace_id: workspaceId },
        body,
        throwOnError: true,
      })
      if (!data) throw new Error('Create label response was empty.')
      return data
    },
    onSuccess: (label: LabelRecord) => {
      queryClient.setQueryData(queryKeys.labels(workspaceId), (current: LabelRecord[] | undefined) =>
        current ? [...current, label] : [label],
      )
      void queryClient.invalidateQueries({ queryKey: queryKeys.labels(workspaceId) })
    },
  })
}
