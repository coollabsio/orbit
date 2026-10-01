import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { apiClient } from '@/api/client'
import { getShortcuts, putShortcuts } from '@/api/generated/sdk.gen'
import { queryKeys } from '@/api/queryKeys'
import type { Overrides } from './bindings'

const NONE: Overrides = {}

/** The signed-in user's shortcut overrides, stored on the server so that they follow the user to every browser.
 *  Until they load, and if they cannot load, the defaults are in effect. */
export function useShortcutBindings() {
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: queryKeys.shortcuts,
    queryFn: async (): Promise<Overrides> => {
      const { data } = await getShortcuts({ client: apiClient, throwOnError: true })
      return data?.bindings ?? NONE
    },
    staleTime: 60_000,
  })
  const mutation = useMutation<unknown, Error, Overrides, { previous: Overrides | undefined }>({
    mutationFn: (bindings) => putShortcuts({ client: apiClient, body: { bindings }, throwOnError: true }),
    // the new keys work at once; a refused save puts the old ones back
    onMutate: async (bindings) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.shortcuts })
      const previous = queryClient.getQueryData<Overrides>(queryKeys.shortcuts)
      queryClient.setQueryData(queryKeys.shortcuts, bindings)
      return { previous }
    },
    onError: (_error, _bindings, context) => {
      queryClient.setQueryData(queryKeys.shortcuts, context?.previous ?? NONE)
      toast.error('Could not save the shortcut. Try again.')
    },
  })
  return { overrides: query.data ?? NONE, save: mutation.mutate, isSaving: mutation.isPending }
}
