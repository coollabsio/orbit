import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { deleteGithubProjectConnection, githubProjectSettings, githubWorkspaceSettings, saveGithubProjectConnection, startGithubManifest } from '@/api/generated/sdk.gen'
import type { GithubProjectConnectionBody } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'

const key = (workspaceId: string, projectId: string) => [...queryKeys.workspace(workspaceId), 'github-project', projectId] as const

export function useGithubWorkspaceSettings(workspaceId: string) {
  return useQuery({
    queryKey: [...queryKeys.workspace(workspaceId), 'github-workspace'],
    queryFn: async () => {
      const { data } = await githubWorkspaceSettings({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('GitHub settings response was empty.')
      return data
    },
  })
}

export function useGithubProjectSettings(workspaceId: string, projectId: string) {
  return useQuery({
    queryKey: key(workspaceId, projectId),
    queryFn: async () => {
      const { data } = await githubProjectSettings({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, throwOnError: true })
      if (!data) throw new Error('GitHub settings response was empty.')
      return data
    },
  })
}

export function useStartGithubManifest(workspaceId: string) {
  return useMutation({
    mutationFn: async (organization: string) => {
      const { data } = await startGithubManifest({ client: apiClient, path: { workspace_id: workspaceId }, body: { organization: organization || null }, throwOnError: true })
      if (!data) throw new Error('GitHub registration response was empty.')
      return data
    },
  })
}

export function useSaveGithubProjectConnection(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: GithubProjectConnectionBody) => {
      const { data } = await saveGithubProjectConnection({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, body, throwOnError: true })
      if (!data) throw new Error('GitHub settings response was empty.')
      return data
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: key(workspaceId, projectId) }),
  })
}

export function useDeleteGithubProjectConnection(workspaceId: string, projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async () => {
      await deleteGithubProjectConnection({ client: apiClient, path: { workspace_id: workspaceId, project_id: projectId }, throwOnError: true })
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: key(workspaceId, projectId) }),
  })
}
