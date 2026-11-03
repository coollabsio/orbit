import { getBilling } from '@orbit/api/generated/sdk.gen'
import type { BillingView } from '@orbit/api/generated/types.gen'

import type { OrbitClient } from './orbit'

const DAY_MILLIS = 24 * 60 * 60 * 1000

export const billingKey = (workspaceId: string) => ['billing', workspaceId] as const

export async function workspaceBilling(client: OrbitClient, workspaceId: string): Promise<BillingView> {
  return (await getBilling({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data
}

export type BillingBanner = { kind: 'trial' | 'read_only'; text: string }

/** The web's `ReadOnlyBanner` text without its billing link: the trial's days left (at least 1), or the read-only line; nothing with full access. */
export function billingBanner(view: BillingView | undefined, now: number): BillingBanner | null {
  if (!view) return null
  if (view.access === 'trial' && view.trial) {
    const days = Math.max(1, Math.ceil((Date.parse(view.trial.ends_at) - now) / DAY_MILLIS))
    return { kind: 'trial', text: `Trial: ${days} ${days === 1 ? 'day' : 'days'} left` }
  }
  if (view.access === 'read_only') return { kind: 'read_only', text: 'This workspace is read-only.' }
  return null
}
