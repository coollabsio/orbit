import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { listImportPeople, updateImportPerson } from '@orbit/api/generated/sdk.gen'

/** People that Asana, Linear and Trello imports could not assign. Admins only. */
export function useImportPeople(workspaceId: string, enabled = true) {
  return useQuery({
    queryKey: queryKeys.importPeople(workspaceId),
    enabled,
    queryFn: async () => {
      const { data } = await listImportPeople({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Import people response was empty.')
      return data.items
    },
  })
}

/** Gives all tasks of a person to a member (`null` takes them off again). */
export function useMapImportPerson(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ personId, userId }: { personId: string; userId: string | null }) => {
      const { data } = await updateImportPerson({ client: apiClient, path: { workspace_id: workspaceId, person_id: personId }, body: { user_id: userId }, throwOnError: true })
      if (!data) throw new Error('Import person response was empty.')
      return data
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.importPeople(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
    },
  })
}
