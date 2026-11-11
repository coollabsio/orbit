import { useQueryClient } from '@tanstack/react-query'
import { pressDatabaseButton } from '@orbit/api/generated/sdk.gen'
import type { DatabaseButtonPress } from '@orbit/api/generated/types.gen'
import { dayOf } from '@orbit/database/dates'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { useDatabaseMutation } from './api'

export type PressInput = { rowId: string; propertyId: string }

/**
 * Presses a button (spec P3 17.3): the server runs its actions in one transaction. The pressed row comes back fresh,
 * the database refetches for the rows it added, other databases it added rows to refetch too, and the client opens
 * the button's URL last.
 */
export function usePressButton(workspaceId: string, databaseId: string) {
  const queryClient = useQueryClient()
  const mutation = useDatabaseMutation<PressInput, DatabaseButtonPress>(workspaceId, databaseId, {
    request: async (path, { rowId, propertyId }) => {
      const { data } = await pressDatabaseButton({
        client: apiClient,
        path: { ...path, row_id: rowId, property_id: propertyId },
        body: { today: dayOf(new Date()) },
        throwOnError: true,
      })
      if (!data) throw new Error('Press button response was empty.')
      return data
    },
    reconcile: (detail, press) => ({ ...detail, rows: detail.rows.map((row) => (row.id === press.row.id ? press.row : row)) }),
    refetch: true,
    error: 'Could not run the button.',
  })
  /** `opensUrl`: the button has an `open_url` action, so a tab opens during the click (later, a popup blocker stops it). */
  const press = (input: PressInput, opensUrl: boolean) => {
    const tab = opensUrl ? window.open('about:blank', '_blank') : null
    if (tab) tab.opener = null
    mutation.mutate(input, {
      onSuccess: (result) => {
        const others = new Set(result.created_rows.map((created) => created.database_id).filter((id) => id !== databaseId))
        for (const id of others) void queryClient.invalidateQueries({ queryKey: queryKeys.database(workspaceId, id) })
        if (result.open_url && tab) tab.location.href = result.open_url
        else tab?.close()
      },
      onError: () => tab?.close(),
    })
  }
  return { press, pending: mutation.isPending }
}
