import { useEffect, useState } from 'react'
import { cn } from 'cn'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Navigate, useSearchParams } from 'react-router'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { SettingsCard } from '@/components/common/SettingsCard'
import type { BillingView, PlanInterval, PlanView, SubscriptionStatus, WorkspaceAccess } from '@orbit/api/generated/types.gen'
import { useBillingCheckout, useBillingEnabled, useBillingPortal, useWorkspaceBilling } from '@/features/workspaces/billing'
import { useAuthOptions } from '@/features/auth/api'
import { fullDate } from '@/lib/format'
import { loadFailed } from '@/lib/connection'

const STATUS_LABELS: Record<SubscriptionStatus, string> = {
  active: 'Active',
  trialing: 'Trial',
  past_due: 'Payment failed, retrying',
  incomplete: 'Waiting for payment',
  incomplete_expired: 'Payment expired',
  canceled: 'Cancelled',
  unpaid: 'Unpaid',
  paused: 'Paused',
}

const ACCESS_LABELS: Record<WorkspaceAccess, string> = {
  full: 'Full access',
  trial: 'Trial',
  read_only: 'Read-only',
}

const INTERVAL_LABELS: Record<PlanInterval, string> = { month: 'Monthly', year: 'Annual' }

/** A price in minor units, with as many decimals as it needs (`$10`, `$8.33`). */
function money(minor: number, currency: string) {
  const format = new Intl.NumberFormat(undefined, { style: 'currency', currency, minimumFractionDigits: 0 })
  return format.format(minor / 10 ** (format.resolvedOptions().maximumFractionDigits ?? 2))
}

/** One entry per product, in config order: the same plan billed at different intervals. */
function groupByProduct(plans: PlanView[]) {
  const products = new Map<string, PlanView[]>()
  for (const plan of plans) {
    const id = plan.product_id ?? plan.key
    products.set(id, [...(products.get(id) ?? []), plan])
  }
  return [...products.values()]
}

function priced(plans: PlanView[], interval: PlanInterval) {
  return plans.find((plan) => plan.interval === interval && plan.amount != null && plan.currency)
}

/** The smallest whole-percent saving of annual over 12 monthly payments across products, so the label holds for each. */
function annualSaving(products: PlanView[][]) {
  const savings = products.flatMap((plans) => {
    const monthly = priced(plans, 'month')
    const annual = priced(plans, 'year')
    if (!monthly?.amount || !annual?.amount || monthly.currency !== annual.currency) return []
    return [Math.floor((1 - annual.amount / (12 * monthly.amount)) * 100)]
  })
  const saving = savings.length ? Math.min(...savings) : 0
  return saving > 0 ? saving : null
}

function subscribedPlan(billing: BillingView) {
  const plan = billing.plans.find((candidate) => candidate.key === billing.plan)
  if (!plan) return billing.plan
  return plan.interval ? `${plan.name} · ${INTERVAL_LABELS[plan.interval]}` : plan.name
}

function PlanCard({ plan, onChoose, disabled }: { plan: PlanView; onChoose?: () => void; disabled: boolean }) {
  const price = plan.amount != null && plan.currency ? { amount: plan.amount, currency: plan.currency } : null
  return (
    <div data-slot="plan-card" className="flex flex-col gap-3 rounded-lg border p-4">
      <h4 className="text-sm font-medium">{plan.name}</h4>
      {price ? (
        <div className="flex flex-col gap-1">
          <p className="flex items-baseline gap-1.5">
            <span className="text-2xl font-semibold tracking-tight">{money(price.amount, price.currency)}</span>
            {plan.interval ? <span className="text-xs text-muted-foreground">per workspace / {plan.interval}</span> : null}
          </p>
          {plan.interval === 'year' ? (
            <p className="text-xs text-muted-foreground">{money(price.amount / 12, price.currency)} / month, billed yearly</p>
          ) : null}
          <p className="text-xs text-muted-foreground">Unlimited tasks, pages and members</p>
        </div>
      ) : null}
      {onChoose ? (
        <Button className="mt-auto w-full" disabled={disabled} onClick={onChoose}>
          Choose {plan.name}
        </Button>
      ) : null}
    </div>
  )
}

function PlanChooser({ plans, onChoose, disabled }: { plans: PlanView[]; onChoose?: (key: string) => void; disabled: boolean }) {
  const [period, setPeriod] = useState<PlanInterval>('month')
  const products = groupByProduct(plans)
  const switchable = products.some((group) => priced(group, 'month') && priced(group, 'year'))
  const saving = annualSaving(products)
  return (
    <div className="flex flex-col gap-4">
      {switchable ? (
        <ToggleGroup
          aria-label="Billing interval"
          variant="outline"
          size="sm"
          spacing={0}
          value={[period]}
          onValueChange={(value: string[]) => {
            const next = value[0]
            if (next === 'month' || next === 'year') setPeriod(next)
          }}
        >
          <ToggleGroupItem value="month">Monthly</ToggleGroupItem>
          <ToggleGroupItem value="year">{saving ? `Annual · save ${saving}%` : 'Annual'}</ToggleGroupItem>
        </ToggleGroup>
      ) : null}
      <div className={cn('grid gap-3 sm:grid-cols-2', products.length > 2 && 'lg:grid-cols-3')}>
        {products.map((group) => {
          const plan = group.find((candidate) => candidate.interval === period) ?? group[0]
          return <PlanCard key={plan.key} plan={plan} disabled={disabled} onChoose={onChoose ? () => onChoose(plan.key) : undefined} />
        })}
      </div>
    </div>
  )
}

function renewal(billing: BillingView) {
  if (!billing.current_period_end || billing.access !== 'full') return null
  const date = fullDate(billing.current_period_end)
  return billing.cancel_at_period_end ? `Ends on ${date}` : `Renews on ${date}`
}

/** Orbit Cloud: the workspace's plan. The owner subscribes through Stripe Checkout and manages it in the Billing Portal. */
export function BillingPage() {
  const options = useAuthOptions()
  const enabled = useBillingEnabled()
  const query = useWorkspaceBilling()
  const checkout = useBillingCheckout()
  const portal = useBillingPortal()
  const [params, setParams] = useSearchParams()
  const returned = params.get('checkout')
  // Back from Stripe Checkout: keep watching for the subscription after the query parameter is gone.
  const [confirming] = useState(() => returned === 'success')

  useEffect(() => {
    if (!returned) return
    // The id keeps it to one toast when the effect runs again before the parameter is gone (StrictMode).
    if (returned === 'success') toast.success('Thanks! Your subscription starts as soon as Stripe confirms the payment.', { id: 'billing-checkout' })
    setParams({}, { replace: true })
  }, [returned, setParams])

  // Stripe confirms through the webhook a moment after the redirect: look again until access arrives.
  const waiting = confirming && query.data?.access !== 'full'
  const { refetch } = query
  useEffect(() => {
    if (!waiting) return
    const timer = window.setInterval(() => void refetch(), 3000)
    const stop = window.setTimeout(() => window.clearInterval(timer), 60_000)
    return () => {
      window.clearInterval(timer)
      window.clearTimeout(stop)
    }
  }, [waiting, refetch])

  if (options.data && !enabled) return <Navigate to="/settings" replace />
  if (loadFailed(query)) return <SettingsCard title="Billing">Billing is not available right now.</SettingsCard>
  if (query.data === undefined) return <SettingsCard title="Billing">Loading billing.</SettingsCard>
  const billing = query.data
  const trial = billing.trial
  const error = checkout.error ?? portal.error
  const busy = checkout.isPending || portal.isPending

  return (
    <>
      <SettingsCard
        title="Billing"
        description={
          billing.exempt
            ? 'This workspace is included with Orbit and needs no plan.'
            : billing.trial_days > 0
              ? `Each workspace has its own plan. A new workspace starts with a ${billing.trial_days}-day trial. Without an active plan after it, the workspace is read-only: everything stays readable and exportable.`
              : 'Each workspace has its own plan. Without an active plan, the workspace is read-only: everything stays readable and exportable.'
        }
        actions={
          billing.can_manage && billing.has_customer ? (
            <Button variant="outline" disabled={busy} onClick={() => portal.mutate()}>
              Manage billing
            </Button>
          ) : null
        }
      >
        <dl className="grid gap-3 text-sm sm:grid-cols-[160px_minmax(0,1fr)]">
          <dt className="text-muted-foreground">Plan</dt>
          <dd>{billing.plan ? subscribedPlan(billing) : billing.status ? 'Other plan' : billing.exempt ? 'Included' : 'No plan'}</dd>
          <dt className="text-muted-foreground">Status</dt>
          <dd>{billing.status ? STATUS_LABELS[billing.status] : billing.exempt ? 'No subscription needed' : 'Not subscribed'}</dd>
          <dt className="text-muted-foreground">Access</dt>
          <dd data-access={billing.access}>{ACCESS_LABELS[billing.access]}</dd>
          {trial ? (
            <>
              <dt className="text-muted-foreground">Trial ends</dt>
              <dd>{fullDate(trial.ends_at)}</dd>
              <dt className="text-muted-foreground">Trial usage</dt>
              <dd>
                <ul>
                  {(['tasks', 'pages', 'members'] as const).map((noun) => (
                    <li key={noun}>
                      {trial[noun].used} of {trial[noun].limit} {noun}
                    </li>
                  ))}
                </ul>
              </dd>
            </>
          ) : null}
          {renewal(billing) ? (
            <>
              <dt className="text-muted-foreground">{billing.cancel_at_period_end ? 'Ends' : 'Renews'}</dt>
              <dd>{renewal(billing)}</dd>
            </>
          ) : null}
        </dl>
        {error ? (
          <p role="alert" className="mt-3 text-xs text-destructive">
            {error.message}
          </p>
        ) : null}
      </SettingsCard>
      {billing.access !== 'full' ? (
        <SettingsCard title="Choose a plan" description={billing.can_manage ? 'You pay through Stripe. You can change or cancel the plan at any time.' : undefined}>
          <PlanChooser plans={billing.plans} disabled={busy || waiting} onChoose={billing.can_manage ? (plan) => checkout.mutate(plan) : undefined} />
          {billing.can_manage ? null : <p className="mt-3 text-sm text-muted-foreground">Only the workspace owner can choose a plan.</p>}
          {waiting ? <p className="mt-3 text-xs text-muted-foreground">Waiting for Stripe to confirm the payment.</p> : null}
        </SettingsCard>
      ) : null}
    </>
  )
}
