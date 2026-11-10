import { useMutation, useQueryClient } from '@tanstack/react-query'
import { submitDatabaseForm } from '@orbit/api/generated/sdk.gen'
import type { DatabaseRow } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { databaseErrorMessage } from '@orbit/database/errors'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'

export type SubmitInput = { viewId: string; answers: Record<string, unknown> }

/** Sends a form view's answers: the server adds the row (spec P3 18.2), then the database refetches. */
export function useSubmitForm(workspaceId: string, databaseId: string) {
  const queryClient = useQueryClient()
  return useMutation<DatabaseRow, Error, SubmitInput>({
    mutationFn: async ({ viewId, answers }) => {
      const { data } = await submitDatabaseForm({
        client: apiClient,
        path: { workspace_id: workspaceId, database_id: databaseId, view_id: viewId },
        body: { answers },
        throwOnError: true,
      })
      if (!data) throw new Error('Submit form response was empty.')
      return data
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.database(workspaceId, databaseId) }),
  })
}

/** The words for a failed submission, shown under the form. */
export function formErrorMessage(error: unknown): string {
  if (error instanceof ApiProblem) {
    if (error.code === 'database_form_closed') return 'This form no longer takes answers.'
    if (error.code === 'database_form_rate_limited') return 'You sent this form too often. Try again in an hour.'
    if (error.code === 'database_view_not_found') return 'This form was deleted.'
  }
  return databaseErrorMessage(error, 'Could not send the form. Try again.')
}
