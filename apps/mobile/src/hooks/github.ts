/** The member's GitHub connection, over `data/github-api.ts`. */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import * as Linking from 'expo-linking'
import * as WebBrowser from 'expo-web-browser'

import { ApiProblem } from '@orbit/api/problem'

import * as api from '../data/github-api'
import { nativeRedirect } from '../data/sign-in-provider'
import { useOrbit } from '../orbit-context'

export function useGithubConnection() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: api.githubKeys.connection(workspaceId), queryFn: () => api.connection(client, workspaceId) })
}

/** Opens GitHub in the system browser; resolves with the outcome, or `null` when the person closed the browser. */
export function useConnectGithub() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (): Promise<api.ConnectResult | null> => {
      // the sign-in redirect URI: the server already accepts it for native returns
      const returnUrl = nativeRedirect(Linking.createURL('auth/callback'))
      const url = await api.startConnection(client, workspaceId, returnUrl)
      const session = await WebBrowser.openAuthSessionAsync(url, returnUrl)
      if (session.type !== 'success') return null
      const back = api.githubReturn(session.url)
      if (!back) return null
      if (back.kind === 'failed') return 'failed'
      try {
        await api.finishConnection(client, workspaceId, back.token)
        return 'connected'
      } catch (error) {
        return api.finishResult(error instanceof ApiProblem ? error.code : undefined)
      }
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: api.githubKeys.connection(workspaceId) }),
  })
}

export function useDisconnectGithub() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.disconnect(client, workspaceId),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: api.githubKeys.connection(workspaceId) }),
  })
}
