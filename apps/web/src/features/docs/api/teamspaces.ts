import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { createTeamspace, deleteTeamspace, listTeamspaces, updateTeamspace } from '@/api/generated/sdk.gen'
import type { CreateTeamspaceBody, Teamspace } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { queryKeys } from '@/api/queryKeys'

type ApiClient = ReturnType<typeof createApiClient>

/** Only workspace owners and admins may delete a teamspace (the server answers 403 for members). */
export function canDeleteTeamspaces(role: string | null | undefined): boolean {
  return role === 'owner' || role === 'admin'
}

/** A user-facing sentence for a failed teamspace delete. */
export function teamspaceDeleteError(error: unknown): string {
  if (error instanceof ApiProblem) {
    if (error.code === 'teamspace_not_empty') return 'Move or delete its pages before deleting this teamspace.'
    if (error.code === 'last_teamspace') return 'A workspace keeps at least one teamspace.'
    if (error.status === 403) return 'Only workspace owners and admins can delete teamspaces.'
    if (error.status === 404) return 'This teamspace no longer exists.'
    if (error.status === 409) return 'This teamspace changed. Try again.'
  }
  return 'Could not delete the teamspace.'
}

function sortTeamspaces(teamspaces: Teamspace[]): Teamspace[] {
  return [...teamspaces].sort((a, b) => a.position - b.position || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
}

export function teamspacesQueryOptions(workspaceId: string, client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.teamspaces(workspaceId),
    queryFn: async () => {
      const { data } = await listTeamspaces({ client, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Teamspaces response was empty.')
      return data.items
    },
  }
}

export function useTeamspaces(workspaceId: string, enabled = true) {
  return useQuery({ ...teamspacesQueryOptions(workspaceId), enabled })
}

export function useCreateTeamspace(workspaceId: string) {
  const queryClient = useQueryClient()
  const key = queryKeys.teamspaces(workspaceId)
  return useMutation<Teamspace, Error, CreateTeamspaceBody>({
    mutationFn: async (body) => {
      const { data } = await createTeamspace({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })
      if (!data) throw new Error('Create teamspace response was empty.')
      return data
    },
    onSuccess: (teamspace) => {
      queryClient.setQueryData(key, (current: Teamspace[] | undefined) =>
        sortTeamspaces([...(current ?? []).filter((item) => item.id !== teamspace.id), teamspace]),
      )
      void queryClient.invalidateQueries({ queryKey: key })
    },
  })
}

export function useRenameTeamspace(workspaceId: string) {
  const queryClient = useQueryClient()
  const key = queryKeys.teamspaces(workspaceId)
  return useMutation<Teamspace, Error, { teamspaceId: string; version: number; name: string }>({
    mutationFn: async ({ teamspaceId, version, name }) => {
      const { data } = await updateTeamspace({
        client: apiClient,
        path: { workspace_id: workspaceId, teamspace_id: teamspaceId },
        body: { expected_version: version, name },
        throwOnError: true,
      })
      if (!data) throw new Error('Update teamspace response was empty.')
      return data
    },
    onSuccess: (teamspace) => {
      queryClient.setQueryData(key, (current: Teamspace[] | undefined) =>
        current?.map((item) => (item.id === teamspace.id ? teamspace : item)),
      )
    },
    // A stale version (409) or a deleted teamspace (404): show the server's list.
    onError: () => void queryClient.invalidateQueries({ queryKey: key }),
  })
}

export function useDeleteTeamspace(workspaceId: string) {
  const queryClient = useQueryClient()
  const key = queryKeys.teamspaces(workspaceId)
  return useMutation<void, Error, { teamspaceId: string; version: number }>({
    mutationFn: async ({ teamspaceId, version }) => {
      await deleteTeamspace({
        client: apiClient,
        path: { workspace_id: workspaceId, teamspace_id: teamspaceId },
        query: { expected_version: version },
        throwOnError: true,
      })
    },
    onSuccess: (_result, { teamspaceId }) => {
      queryClient.setQueryData(key, (current: Teamspace[] | undefined) => current?.filter((item) => item.id !== teamspaceId))
      void queryClient.invalidateQueries({ queryKey: key })
      // Its trashed pages are gone too, and another teamspace may be the default now.
      void queryClient.invalidateQueries({ queryKey: queryKeys.pages.all(workspaceId) })
    },
    onError: (error) => {
      if (error instanceof ApiProblem && (error.status === 404 || (error.status === 409 && error.code !== 'teamspace_not_empty'))) {
        void queryClient.invalidateQueries({ queryKey: key })
      }
    },
  })
}
