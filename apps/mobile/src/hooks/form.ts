import { useMutation, useQueryClient } from '@tanstack/react-query'

import { submitForm } from '../data/form-api'
import { keys } from '../data/keys'
import { useOrbit } from '../orbit-context'

/** Sends a form view's answers, then refreshes the database (its own views and every linked view). */
export function useSubmitForm(databaseId: string) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ viewId, answers }: { viewId: string; answers: Record<string, unknown> }) => submitForm(client, workspaceId, databaseId, viewId, answers),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.database(workspaceId, databaseId) }),
  })
}
