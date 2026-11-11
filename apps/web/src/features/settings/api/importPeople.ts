import { useEffect, useRef } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ImportPerson } from '@orbit/api/generated/types.gen'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { listImportPeople, updateImportPerson } from '@orbit/api/generated/sdk.gen'

/** Shared by the list and the mapping, so the list shows a mapping at once. */
function importPeopleKey(workspaceId: string) {
  return queryKeys.importPeople(workspaceId)
}

/**
 * People that Asana, Linear and Trello imports could not assign. Admins only. Polled while a job moves someone's
 * tasks; when it ends the task lists are refreshed.
 */
export function useImportPeople(workspaceId: string, enabled = true) {
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: importPeopleKey(workspaceId),
    enabled,
    queryFn: async () => {
      const { data } = await listImportPeople({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })
      if (!data) throw new Error('Import people response was empty.')
      return data.items
    },
    refetchInterval: (current) => (current.state.data?.some((person) => person.assigning) ? 2000 : false),
  })
  const assigning = (query.data ?? []).filter((person) => person.assigning).map((person) => person.id).join(',')
  const previous = useRef(assigning)
  useEffect(() => {
    const before = previous.current
    previous.current = assigning
    if (before && before !== assigning) void queryClient.invalidateQueries({ queryKey: queryKeys.tasks.all(workspaceId) })
  }, [assigning, queryClient, workspaceId])
  return query
}

/** Maps a person to a member (`null` takes them off again); a job then moves their tasks. */
export function useMapImportPerson(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ personId, userId }: { personId: string; userId: string | null }) => {
      const { data } = await updateImportPerson({ client: apiClient, path: { workspace_id: workspaceId, person_id: personId }, body: { user_id: userId }, throwOnError: true })
      if (!data) throw new Error('Import person response was empty.')
      return data
    },
    onSuccess: (person) => {
      queryClient.setQueryData<ImportPerson[]>(importPeopleKey(workspaceId), (people) => people?.map((item) => (item.id === person.id ? person : item)))
      void queryClient.invalidateQueries({ queryKey: importPeopleKey(workspaceId) })
    },
  })
}
