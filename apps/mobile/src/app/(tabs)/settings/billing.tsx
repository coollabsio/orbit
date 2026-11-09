import type { BillingView, PlanInterval, PlanView } from '@orbit/api/generated/types.gen'
import { Redirect } from 'expo-router'
import * as WebBrowser from 'expo-web-browser'
import { type ReactNode, useEffect, useState } from 'react'
import { View } from 'react-native'

import { Note, SettingsCard, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { Toggles } from '../../../components/settings/Toggles'
import { ACCESS_LABELS, annualSaving, billingDescription, groupByProduct, money, planLabel, priced, renewal, STATUS_LABELS } from '../../../data/billing'
import { fullDate } from '../../../data/format'
import { useAuthOptions } from '../../../hooks/account'
import { useBillingLinks, useWorkspaceBilling } from '../../../hooks/workspace-settings'
import { errorMessage } from '../../../session-context'
import { Button, Text } from '../../../ui'

/** Stripe confirms through its webhook a moment after checkout: look again every few seconds, for a minute. */
const CONFIRM_POLL_MS = 3000
const CONFIRM_POLL_FOR_MS = 60_000

/** Orbit Cloud: the workspace's plan. The owner subscribes through Stripe Checkout and manages it in the Billing Portal, both in the browser. */
export default function BillingScreen() {
  const options = useAuthOptions()
  const enabled = options.data?.billing_enabled ?? false
  const query = useWorkspaceBilling()
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={() => query.refetch()}>
      {options.data && !enabled ? (
        <Redirect href="/settings" />
      ) : query.isError ? (
        <SettingsCard title="Billing">
          <BodyText>Billing is not available right now.</BodyText>
        </SettingsCard>
      ) : query.data === undefined ? (
        <SettingsCard title="Billing">
          <BodyText>Loading billing.</BodyText>
        </SettingsCard>
      ) : (
        <Billing billing={query.data} refetch={query.refetch} />
      )}
    </SettingsShell>
  )
}

function Billing({ billing, refetch }: { billing: BillingView; refetch: () => Promise<unknown> }) {
  const { checkout, portal } = useBillingLinks()
  // back from Stripe Checkout: keep looking for the subscription until access arrives
  const [confirming, setConfirming] = useState(false)
  const waiting = confirming && billing.access !== 'full'
  useEffect(() => {
    if (!waiting) return
    const timer = setInterval(() => void refetch(), CONFIRM_POLL_MS)
    const stop = setTimeout(() => clearInterval(timer), CONFIRM_POLL_FOR_MS)
    return () => {
      clearInterval(timer)
      clearTimeout(stop)
    }
  }, [waiting, refetch])

  const open = async (url: string) => {
    await WebBrowser.openBrowserAsync(url)
    await refetch()
  }
  const choose = (plan: string) =>
    checkout.mutate(plan, {
      onSuccess: (url) => {
        setConfirming(true)
        void open(url)
      },
    })
  const error = checkout.error ?? portal.error
  const busy = checkout.isPending || portal.isPending
  const trial = billing.trial
  const renews = renewal(billing)

  return (
    <>
      <SettingsCard
        title="Billing"
        description={billingDescription(billing)}
        actions={billing.can_manage && billing.has_customer ? <Button label="Manage billing" variant="outline" disabled={busy} onPress={() => portal.mutate(undefined, { onSuccess: (url) => void open(url) })} /> : null}
      >
        {/* the web's `dl` at phone size: each term above its value */}
        <View className="gap-3">
          <Term label="Plan">{planLabel(billing)}</Term>
          <Term label="Status">{billing.status ? STATUS_LABELS[billing.status] : billing.exempt ? 'No subscription needed' : 'Not subscribed'}</Term>
          <Term label="Access">{ACCESS_LABELS[billing.access]}</Term>
          {trial ? (
            <>
              <Term label="Trial ends">{fullDate(trial.ends_at)}</Term>
              <Term label="Trial usage">
                {(['tasks', 'pages', 'members'] as const).map((noun) => `${trial[noun].used} of ${trial[noun].limit} ${noun}`).join('\n')}
              </Term>
            </>
          ) : null}
          {renews ? <Term label={billing.cancel_at_period_end ? 'Ends' : 'Renews'}>{renews}</Term> : null}
        </View>
        {error ? (
          <View className="mt-3">
            <Note error>{errorMessage(error)}</Note>
          </View>
        ) : null}
      </SettingsCard>
      {billing.access !== 'full' ? (
        <SettingsCard title="Choose a plan" description={billing.can_manage ? 'You pay through Stripe. You can change or cancel the plan at any time.' : undefined}>
          <PlanChooser plans={billing.plans} disabled={busy || waiting} onChoose={billing.can_manage ? choose : undefined} />
          {billing.can_manage ? null : <BodyText className="mt-3 text-muted-foreground">Only the workspace owner can choose a plan.</BodyText>}
          {waiting ? <Text className="mt-3 text-[12px] leading-[16px] text-muted-foreground">Waiting for Stripe to confirm the payment.</Text> : null}
        </SettingsCard>
      ) : null}
    </>
  )
}

/** The web card body's `text-sm`. */
function BodyText({ children, className = 'text-foreground' }: { children: ReactNode; className?: string }) {
  return <Text className={`text-[14px] leading-[20px] ${className}`}>{children}</Text>
}

function Term({ label, children }: { label: string; children: ReactNode }) {
  return (
    <View className="gap-3">
      <BodyText className="text-muted-foreground">{label}</BodyText>
      <BodyText>{children}</BodyText>
    </View>
  )
}

function PlanChooser({ plans, onChoose, disabled }: { plans: PlanView[]; onChoose?: (key: string) => void; disabled: boolean }) {
  const [period, setPeriod] = useState<PlanInterval>('month')
  const products = groupByProduct(plans)
  const switchable = products.some((group) => priced(group, 'month') && priced(group, 'year'))
  const saving = annualSaving(products)
  return (
    <View className="gap-4">
      {switchable ? (
        <View className="items-start">
          <Toggles
            label="Billing interval"
            value={period}
            options={[
              { value: 'month', label: 'Monthly' },
              { value: 'year', label: saving ? `Annual · save ${saving}%` : 'Annual' },
            ]}
            onChange={setPeriod}
          />
        </View>
      ) : null}
      <View className="gap-3">
        {products.map((group) => {
          const plan = group.find((candidate) => candidate.interval === period) ?? group[0]
          return <PlanCard key={plan.key} plan={plan} disabled={disabled} onChoose={onChoose ? () => onChoose(plan.key) : undefined} />
        })}
      </View>
    </View>
  )
}

function PlanCard({ plan, onChoose, disabled }: { plan: PlanView; onChoose?: () => void; disabled: boolean }) {
  const price = plan.amount != null && plan.currency ? { amount: plan.amount, currency: plan.currency } : null
  return (
    <View className="gap-3 rounded-lg border border-border p-4">
      <Text className="font-noto-medium text-[14px] leading-[20px]">{plan.name}</Text>
      {price ? (
        <View className="gap-1">
          <View className="flex-row items-baseline gap-1.5">
            <Text className="font-noto-semibold text-[24px] leading-[32px] tracking-tight">{money(price.amount, price.currency)}</Text>
            {plan.interval ? <Text className="text-[12px] leading-[16px] text-muted-foreground">per workspace / {plan.interval}</Text> : null}
          </View>
          {plan.interval === 'year' ? <Text className="text-[12px] leading-[16px] text-muted-foreground">{money(price.amount / 12, price.currency)} / month, billed yearly</Text> : null}
          <Text className="text-[12px] leading-[16px] text-muted-foreground">Unlimited tasks, pages and members</Text>
        </View>
      ) : null}
      {onChoose ? <Button label={`Choose ${plan.name}`} block disabled={disabled} onPress={onChoose} /> : null}
    </View>
  )
}
