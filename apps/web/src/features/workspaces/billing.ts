import { useMutation, useQuery } from '@tanstack/react-query'
import { apiClient, PAYMENT_REQUIRED_EVENT } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { billingCheckout, billingPortal, getAccountTrial, getBilling } from '@orbit/api/generated/sdk.gen'
import type { AccountTrial } from '@orbit/api/generated/types.gen'
import { fullDate } from '@/lib/format'
import { useAuthOptions } from '@/features/auth/api'
import { useWorkspace } from './workspaceContext'

/** Orbit Cloud: workspaces need a subscription to be writable. Off on a self-hosted server. */
export function useBillingEnabled(): boolean {
  return useAuthOptions().data?.billing_enabled ?? false
}

/** The current workspace's subscription; only fetched in Orbit Cloud. */
export function useWorkspaceBilling() {
  const { workspace } = useWorkspace()
  const enabled = useBillingEnabled()
  return useQuery({
    queryKey: queryKeys.billing(workspace.id),
    enabled,
    queryFn: async () => {
      const { data } = await getBilling({ client: apiClient, path: { workspace_id: workspace.id }, throwOnError: true })
      if (!data) throw new Error('Billing response was empty.')
      return data
    },
  })
}

/** Orbit Cloud: the caller's trial, which every workspace they create shares. Not fetched on a self-hosted server. */
export function useAccountTrial() {
  const enabled = useBillingEnabled()
  return useQuery({
    queryKey: queryKeys.accountTrial,
    enabled,
    queryFn: async () => {
      const { data } = await getAccountTrial({ client: apiClient, throwOnError: true })
      if (!data) throw new Error('Trial response was empty.')
      return data
    },
  })
}

/** What creating one more workspace means for the caller's trial; `null` when there is nothing to say. */
export function newWorkspaceTrialNote(trial: AccountTrial | undefined, now = Date.now()): string | null {
  if (!trial) return null
  if (!trial.ends_at) {
    return trial.days > 0
      ? `This workspace starts your ${trial.days}-day free trial. Later workspaces share it.`
      : 'This workspace needs a plan before you can add to it.'
  }
  const date = fullDate(trial.ends_at)
  return Date.parse(trial.ends_at) > now
    ? `This workspace shares your free trial, which ends on ${date}.`
    : `Your free trial ended on ${date}. This workspace stays read-only until it has a plan.`
}

/** The workspace has no subscription that grants access: it can be read but not changed. */
export function useWorkspaceReadOnly(): boolean {
  return useWorkspaceBilling().data?.access === 'read_only'
}

/**
 * A co-editing socket closed because the workspace became read-only (close 4402). The same event as a refused write:
 * one toast, and the billing state is fetched again, so the editors turn read-only before they reconnect.
 */
export function announceWorkspaceReadOnly(): void {
  const detail = 'This workspace became read-only because it has no active subscription. Your editor is now read-only.'
  window.dispatchEvent(new CustomEvent(PAYMENT_REQUIRED_EVENT, { detail }))
}

/** Starts a Stripe Checkout for `plan` and sends the browser there. */
export function useBillingCheckout() {
  const { workspace } = useWorkspace()
  return useMutation({
    mutationFn: async (plan: string) => {
      const { data } = await billingCheckout({ client: apiClient, path: { workspace_id: workspace.id }, body: { plan }, throwOnError: true })
      if (!data) throw new Error('Checkout response was empty.')
      return data.url
    },
    onSuccess: (url) => window.location.assign(url),
  })
}

/** Opens the Stripe Billing Portal. */
export function useBillingPortal() {
  const { workspace } = useWorkspace()
  return useMutation({
    mutationFn: async () => {
      const { data } = await billingPortal({ client: apiClient, path: { workspace_id: workspace.id }, throwOnError: true })
      if (!data) throw new Error('Billing portal response was empty.')
      return data.url
    },
    onSuccess: (url) => window.location.assign(url),
  })
}
