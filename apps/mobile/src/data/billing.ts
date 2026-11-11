import { billingCheckout, billingPortal, getBilling } from '@orbit/api/generated/sdk.gen'
import type { BillingView, PlanInterval, PlanView, SubscriptionStatus, WorkspaceAccess } from '@orbit/api/generated/types.gen'

import { fullDate } from './format'
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

/** A Stripe Checkout for `plan`: the URL to open. */
export async function billingCheckoutUrl(client: OrbitClient, workspaceId: string, plan: string): Promise<string> {
  return (await billingCheckout({ client, path: { workspace_id: workspaceId }, body: { plan }, throwOnError: true })).data.url
}

/** The Stripe Billing Portal: the URL to open. */
export async function billingPortalUrl(client: OrbitClient, workspaceId: string): Promise<string> {
  return (await billingPortal({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data.url
}

export const STATUS_LABELS: Record<SubscriptionStatus, string> = {
  active: 'Active',
  trialing: 'Trial',
  past_due: 'Payment failed, retrying',
  incomplete: 'Waiting for payment',
  incomplete_expired: 'Payment expired',
  canceled: 'Cancelled',
  unpaid: 'Unpaid',
  paused: 'Paused',
}

export const ACCESS_LABELS: Record<WorkspaceAccess, string> = { full: 'Full access', trial: 'Trial', read_only: 'Read-only' }

const INTERVAL_LABELS: Record<PlanInterval, string> = { month: 'Monthly', year: 'Annual' }

/** A price in minor units, with as many decimals as it needs (`$10`, `$8.33`). */
export function money(minor: number, currency: string): string {
  const format = new Intl.NumberFormat(undefined, { style: 'currency', currency, minimumFractionDigits: 0 })
  return format.format(minor / 10 ** (format.resolvedOptions().maximumFractionDigits ?? 2))
}

/** One entry per product, in config order: the same plan billed at different intervals. */
export function groupByProduct(plans: readonly PlanView[]): PlanView[][] {
  const products = new Map<string, PlanView[]>()
  for (const plan of plans) {
    const id = plan.product_id ?? plan.key
    products.set(id, [...(products.get(id) ?? []), plan])
  }
  return [...products.values()]
}

export function priced(plans: readonly PlanView[], interval: PlanInterval): PlanView | undefined {
  return plans.find((plan) => plan.interval === interval && plan.amount != null && plan.currency)
}

/** The whole-percent saving of one product's annual plan over 12 monthly payments. */
export function annualSaving(plans: readonly PlanView[]): number | null {
  const monthly = priced(plans, 'month')
  const annual = priced(plans, 'year')
  if (!monthly?.amount || !annual?.amount || monthly.currency !== annual.currency) return null
  const saving = Math.floor((1 - annual.amount / (12 * monthly.amount)) * 100)
  return saving > 0 ? saving : null
}

/** A plan's name with its interval: `Team · Annual`. */
export function planTitle(plan: PlanView): string {
  return planTitle(plan)
}

/** The label of a plan card's button: `Choose annual`, or `Choose Team` without an interval. */
export function chooseLabel(plan: PlanView): string {
  return plan.interval ? `Choose ${INTERVAL_LABELS[plan.interval].toLowerCase()}` : `Choose ${plan.name}`
}

/** The Plan line: the subscribed plan and its interval, or what stands in for one. */
export function planLabel(billing: BillingView): string {
  if (!billing.plan) return billing.status ? 'Other plan' : billing.exempt ? 'Included' : 'No plan'
  const plan = billing.plans.find((candidate) => candidate.key === billing.plan)
  if (!plan) return billing.plan
  return planTitle(plan)
}

/** `Renews on …` / `Ends on …` for a paid plan with full access, else `null`. */
export function renewal(billing: BillingView): string | null {
  if (!billing.current_period_end || billing.access !== 'full') return null
  const date = fullDate(billing.current_period_end)
  return billing.cancel_at_period_end ? `Ends on ${date}` : `Renews on ${date}`
}

/** The Billing card's explanation (its title's tooltip on the web). */
export function billingDescription(billing: BillingView): string {
  if (billing.exempt) return 'This workspace is included with Orbit and needs no plan.'
  if (billing.trial_days > 0) {
    return `Each workspace has its own plan. A new workspace starts with a ${billing.trial_days}-day trial. Without an active plan after it, the workspace is read-only: everything stays readable and exportable.`
  }
  return 'Each workspace has its own plan. Without an active plan, the workspace is read-only: everything stays readable and exportable.'
}
