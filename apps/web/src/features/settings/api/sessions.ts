import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '../../../api/client'
import { queryKeys } from '../../../api/queryKeys'
import { listSessions, revokeSession } from '../../../api/generated/sdk.gen'
import type { SessionRecord } from '../../../api/generated/types.gen'

export interface SessionView extends SessionRecord {
  current: boolean
  device: string
  browser: string
}

export function sessionView(session: SessionRecord): SessionView {
  return { ...session, device: 'Signed-in device', browser: 'Orbit web' }
}

export function useSessions() {
  return useQuery({
    queryKey: queryKeys.sessions,
    queryFn: async () => {
      const { data } = await listSessions({ client: apiClient, throwOnError: true })
      if (!data) throw new Error('Sessions response was empty.')
      return data.map(sessionView)
    },
  })
}

export function useRevokeSession() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (sessionId: string) => {
      await revokeSession({ client: apiClient, path: { id: sessionId }, throwOnError: true })
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.sessions }),
  })
}
