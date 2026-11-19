import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { deleteGithubConnection, finishGithubConnection, githubConnection, startGithubConnection } from '@orbit/api/generated/sdk.gen'
import { ApiProblem } from '@orbit/api/problem'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'

/** The signed-in member's GitHub account for PR review in this workspace. */
export function useGithubConnection(workspaceId: string) {
  return useQuery({
    queryKey: queryKeys.github.connection(workspaceId),
    queryFn: async () => {
      const { data } = await githubConnection({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('GitHub connection response was empty.')
      return data
    },
  })
}

/** Starts GitHub's OAuth flow and leaves for GitHub; GitHub returns to `/profile/github?github=…`. */
export function useConnectGithub(workspaceId: string) {
  return useMutation({
    mutationFn: async () => {
      const { data } = await startGithubConnection({ client: apiClient, path: { workspace_id: workspaceId }, body: { client: 'web' }, throwOnError: true })
      if (!data) throw new Error('GitHub connection response was empty.')
      window.location.assign(data.authorize_url)
    },
  })
}

export function useDisconnectGithub(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async () => {
      await deleteGithubConnection({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.github.connection(workspaceId) }),
  })
}

export type FinishResult = 'connected' | 'failed' | 'taken' | 'mismatch'

/** Back from GitHub: connects the account as the signed-in member, with the one-time `token` of the return URL. */
export function useFinishGithub(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (token: string): Promise<FinishResult> => {
      try {
        await finishGithubConnection({ client: apiClient, path: { workspace_id: workspaceId }, body: { token }, throwOnError: true })
        return 'connected'
      } catch (error) {
        if (error instanceof ApiProblem && error.code === 'github_account_taken') return 'taken'
        if (error instanceof ApiProblem && error.code === 'github_connect_mismatch') return 'mismatch'
        return 'failed'
      }
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.github.connection(workspaceId) }),
  })
}
