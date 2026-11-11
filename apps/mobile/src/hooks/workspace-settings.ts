/** The workspace settings queries and writes of Billing, GitHub, API tokens and Export, over the data modules. */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { billingCheckoutUrl, billingKey, billingPortalUrl, workspaceBilling } from '../data/billing'
import * as api from '../data/workspace-settings-api'
import { useOrbit } from '../orbit-context'
import { useAuthOptions } from './account'

/** Polls while an export is being prepared (the web's interval). */
const EXPORT_POLL_MS = 3000

export function useBillingEnabled(): boolean {
  return useAuthOptions().data?.billing_enabled ?? false
}

/** The open workspace's subscription; only fetched in Orbit Cloud. */
export function useWorkspaceBilling() {
  const { client, workspaceId } = useOrbit()
  const enabled = useBillingEnabled()
  return useQuery({ queryKey: billingKey(workspaceId), queryFn: () => workspaceBilling(client, workspaceId), enabled })
}

/** Stripe Checkout or the Billing Portal: each answers with the URL to open. */
export function useBillingLinks() {
  const { client, workspaceId } = useOrbit()
  const checkout = useMutation({ mutationFn: (plan: string) => billingCheckoutUrl(client, workspaceId, plan) })
  const portal = useMutation({ mutationFn: () => billingPortalUrl(client, workspaceId) })
  return { checkout, portal }
}

export function useGithubSettings(enabled = true) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ enabled, queryKey: api.workspaceSettingsKeys.github(workspaceId), queryFn: () => api.githubSettings(client, workspaceId) })
}

export function useApiTokens(enabled: boolean) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: api.workspaceSettingsKeys.apiTokens(workspaceId), queryFn: () => api.apiTokens(client, workspaceId), enabled })
}

export function useApiTokenWrites() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const onSettled = () => void queryClient.invalidateQueries({ queryKey: api.workspaceSettingsKeys.apiTokens(workspaceId) })
  return {
    create: useMutation({ mutationFn: (token: api.NewApiToken) => api.issueApiToken(client, workspaceId, token), onSettled }),
    revoke: useMutation({ mutationFn: (tokenId: string) => api.revokeToken(client, workspaceId, tokenId), onSettled }),
  }
}

/** The workspace's exports, newest first; polls while one is being prepared. */
export function useWorkspaceExports(enabled: boolean) {
  const { client, workspaceId } = useOrbit()
  return useQuery({
    queryKey: api.workspaceSettingsKeys.exports(workspaceId),
    queryFn: () => api.workspaceExports(client, workspaceId),
    enabled,
    refetchInterval: (query) => (query.state.data?.some(api.isActiveExport) ? EXPORT_POLL_MS : false),
  })
}

export function useStartWorkspaceExport() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => api.startWorkspaceExport(client, workspaceId),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: api.workspaceSettingsKeys.exports(workspaceId) }),
  })
}
