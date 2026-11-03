import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { createWorkspaceExport, listWorkspaceExports } from '@orbit/api/generated/sdk.gen'
import type { WorkspaceExport } from '@orbit/api/generated/types.gen'

const POLL_MS = 3000

export const isActiveExport = (item: WorkspaceExport) => item.status === 'queued' || item.status === 'running'

/** The workspace's exports, newest first; polls while one is being prepared. */
export function useWorkspaceExports(workspaceId: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.workspaceExports(workspaceId),
    enabled,
    queryFn: async () => {
      const { data } = await listWorkspaceExports({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Export list response was empty.')
      return data.items
    },
    refetchInterval: (query) => (query.state.data?.some(isActiveExport) ? POLL_MS : false),
  })
}

export function useStartWorkspaceExport(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async () => {
      const { data } = await createWorkspaceExport({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Export response was empty.')
      return data
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.workspaceExports(workspaceId) }),
  })
}
