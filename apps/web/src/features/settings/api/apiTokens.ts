import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '../../../api/client'
import { queryKeys } from '../../../api/queryKeys'
import { createApiToken, listApiTokens, revokeApiToken } from '../../../api/generated/sdk.gen'
import type { ApiTokenScope } from '../../../api/generated/types.gen'

export function useApiTokens(workspaceId: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.apiTokens(workspaceId),
    enabled,
    queryFn: async () => {
      const { data } = await listApiTokens({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('API token response was empty.')
      return data
    },
  })
}

export function useCreateApiToken(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ name, projectId, scopes }: { name: string; projectId: string; scopes: ApiTokenScope[] }) => {
      const { data } = await createApiToken({ client: apiClient, path: { workspace_id: workspaceId }, body: { name, project_id: projectId, scopes }, throwOnError: true })
      if (!data) throw new Error('API token response was empty.')
      return data
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.apiTokens(workspaceId) }),
  })
}

export function useRevokeApiToken(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (tokenId: string) => revokeApiToken({ client: apiClient, path: { workspace_id: workspaceId, token_id: tokenId }, throwOnError: true }),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.apiTokens(workspaceId) }),
  })
}
