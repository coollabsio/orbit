import { useId, useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import type { Limits, MailSendPolicy, SendTier } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { InfoTip } from '@/components/common/InfoTip'
import { Modal } from '@/components/common/Modal'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useResumeSending, useSetSendDefault, useSetSendPolicy } from '@/features/mail/api/mutations'
import { useSendPolicies, useSendRoutes } from '@/features/mail/api/queries'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { count, limitRows, mailProblem, percent, routeLabel, since, TIER_LABELS, TIER_LIMITS } from '@/features/settings/pages/mail/mailSettingsLib'

const NO_DEFAULT = 'none'

/** The instance route that domains without a route of their own use. */
export function DefaultRouteCard() {
  const routes = useSendRoutes('instance')
  const save = useSetSendDefault()
  const own = routes.data?.routes.filter((route) => route.scope === 'instance') ?? []
  const options = [{ value: NO_DEFAULT, label: 'None: such domains cannot send' }, ...own.map((route) => ({ value: route.id, label: `${route.name} (${routeLabel(route.kind)})` }))]
  return (
    <SettingsCard title="Default route" flush>
      <SettingsRow className="flex-nowrap">
        <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
          Domains without a route send through
          <InfoTip text="Workspaces can pick another route for each domain. A route cannot be removed while it is the default." />
        </span>
        <Select
          items={options}
          value={routes.data ? (routes.data.default_route_id ?? NO_DEFAULT) : null}
          disabled={!routes.data || save.isPending}
          onValueChange={(value) => save.mutate(value === NO_DEFAULT ? null : (value as string), {
            onSuccess: () => toast('Default route saved'),
            onError: (error) => toast.error(mailProblem(error, 'The default route could not be saved.')),
          })}
        >
          <SelectTrigger aria-label="Default send route" className="w-72 max-w-[50vw]"><SelectValue placeholder="…" /></SelectTrigger>
          <SelectContent>
            {options.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
          </SelectContent>
        </Select>
      </SettingsRow>
    </SettingsCard>
  )
}

/** The limits of the New and Standard tiers (read only: Orbit's built-in numbers). */
export function TierDefaultsCard() {
  return (
    <SettingsCard
      title="Tier limits"
      description="A new workspace starts on New. Orbit moves it to Standard after 14 days of sending without a pause, with under 2 % hard bounces and under 0.1 % complaints. Sending pauses when, over at least 100 recipients in 7 days, more than 5 % bounce hard or more than 0.3 % complain."
      flush
    >
      <div className="grid grid-cols-1 divide-y sm:grid-cols-2 sm:divide-x sm:divide-y-0">
        {(['new', 'standard'] as const).map((tier) => (
          <div key={tier} className="flex flex-col">
            <div className="px-4 pt-3 text-[0.8125rem] font-medium text-foreground">{TIER_LABELS[tier]}</div>
            {limitRows(TIER_LIMITS[tier]).map((row) => (
              <div key={row.label} className="flex items-center gap-3 px-4 py-1.5 text-[0.8125rem] last:pb-3">
                <span className="min-w-0 flex-1 text-muted-foreground">{row.label}</span>
                <span>{row.value}</span>
              </div>
            ))}
          </div>
        ))}
      </div>
    </SettingsCard>
  )
}

/** Every workspace's tier, use, bounce and complaint rates and pause, with **Resume** and **Set limits**. */
export function SendPoliciesCard() {
  const policies = useSendPolicies()
  const [editing, setEditing] = useState<MailSendPolicy | null>(null)
  return (
    <>
      <SettingsCard title="Workspaces" description="What each workspace sent: recipients in the last hour and day, and its rates over 7 days. Paused workspaces come first." flush>
        {policies.isPending ? <p role="status" className="m-0 p-4 text-sm text-muted-foreground">Loading workspaces…</p> : null}
        {policies.isError ? (
          <p role="alert" className="m-0 p-4 text-sm text-destructive">
            {mailProblem(policies.error, 'The workspaces could not be loaded.')}{' '}
            <Button variant="ghost" onClick={() => void policies.refetch()}>Retry</Button>
          </p>
        ) : null}
        {policies.data?.items.length === 0 ? <p className="m-0 p-4 text-sm text-muted-foreground">No workspace has sent mail yet.</p> : null}
        <ul className="m-0 flex list-none flex-col divide-y p-0">
          {policies.data?.items.map((policy) => <PolicyRow key={policy.workspace_id} policy={policy} onSetLimits={() => setEditing(policy)} />)}
        </ul>
      </SettingsCard>
      {editing ? <LimitsForm policy={editing} onClose={() => setEditing(null)} /> : null}
    </>
  )
}

function PolicyRow({ policy, onSetLimits }: { policy: MailSendPolicy; onSetLimits: () => void }) {
  const resume = useResumeSending()

  async function resumeSending() {
    const confirmed = await confirmAction({
      title: `Resume sending for ${policy.workspace_name}?`,
      description: 'Its held messages go out at the next run. If the rates stay high, sending pauses again.',
      confirmLabel: 'Resume',
    })
    if (!confirmed) return
    resume.mutate(policy.workspace_id, {
      onSuccess: () => toast(`${policy.workspace_name} sends again`),
      onError: (error) => toast.error(mailProblem(error, 'Sending could not be resumed.')),
    })
  }

  return (
    <li className="flex flex-wrap items-start gap-3 px-4 py-3">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[0.8125rem] font-medium text-foreground">{policy.workspace_name}</span>
          <Badge variant="outline">{TIER_LABELS[policy.tier]}</Badge>
          {policy.paused_at ? <Badge variant="destructive">Paused</Badge> : null}
        </div>
        <span className="text-xs text-muted-foreground">
          {policy.use_hour.toLocaleString('en-US')} of {policy.limits.per_ws_hour.toLocaleString('en-US')} this hour · {policy.use_day.toLocaleString('en-US')} of{' '}
          {policy.limits.per_ws_day.toLocaleString('en-US')} today
        </span>
        <span className="text-xs text-muted-foreground">
          7 days: {count(policy.recipients_7d, 'recipient', 'recipients')} · {percent(policy.hard_bounce_rate_7d)} bounced · {percent(policy.complaint_rate_7d)} complained
        </span>
        {policy.paused_at ? (
          <span role="status" className="text-xs text-destructive">
            Paused {since(policy.paused_at)}{policy.pause_reason ? `: ${policy.pause_reason}` : '.'}
          </span>
        ) : null}
      </div>
      <div className="flex items-center gap-1">
        {policy.paused_at ? <Button type="button" variant="outline" size="sm" disabled={resume.isPending} onClick={() => void resumeSending()}>Resume</Button> : null}
        <Button type="button" variant="ghost" size="sm" onClick={onSetLimits}>Set limits</Button>
      </div>
    </li>
  )
}

const TIER_OPTIONS: { value: SendTier; label: string }[] = [
  { value: 'new', label: 'New' },
  { value: 'standard', label: 'Standard' },
  { value: 'custom', label: 'Custom' },
]

const LIMIT_FIELDS: { name: keyof Limits; label: string }[] = [
  { name: 'per_user_hour', label: 'Each member, an hour' },
  { name: 'per_user_day', label: 'Each member, a day' },
  { name: 'per_ws_hour', label: 'Workspace, an hour' },
  { name: 'per_ws_day', label: 'Workspace, a day' },
  { name: 'max_recipients', label: 'Recipients of one message' },
]

const MAX_LIMIT = 10_000_000

/** **Set limits**: a tier, or custom numbers (1 to 10,000,000 each). */
function LimitsForm({ policy, onClose }: { policy: MailSendPolicy; onClose: () => void }) {
  const ids = useId()
  const save = useSetSendPolicy()
  const [tier, setTier] = useState<SendTier>(policy.tier)
  const [limits, setLimits] = useState<Record<keyof Limits, string>>(() => ({
    per_user_hour: String(policy.limits.per_user_hour),
    per_user_day: String(policy.limits.per_user_day),
    per_ws_hour: String(policy.limits.per_ws_hour),
    per_ws_day: String(policy.limits.per_ws_day),
    max_recipients: String(policy.limits.max_recipients),
  }))
  const numbers = Object.fromEntries(LIMIT_FIELDS.map((field) => [field.name, Number(limits[field.name])])) as unknown as Limits
  const valid = tier !== 'custom' || LIMIT_FIELDS.every((field) => Number.isInteger(numbers[field.name]) && numbers[field.name] >= 1 && numbers[field.name] <= MAX_LIMIT)

  function submit(event: FormEvent) {
    event.preventDefault()
    if (!valid || save.isPending) return
    save.mutate(
      { workspaceId: policy.workspace_id, body: { tier, custom_limits: tier === 'custom' ? numbers : null } },
      {
        onSuccess: () => { toast(`Limits of ${policy.workspace_name} saved`); onClose() },
      },
    )
  }

  return (
    <Modal title={`Limits of ${policy.workspace_name}`} onClose={onClose} dismissible={!save.isPending}>
      <form className="grid gap-4" onSubmit={submit}>
        <Field>
          <FieldLabel htmlFor={`${ids}-tier`}>
            Tier
            <InfoTip text="New and Standard use Orbit's numbers. A workspace on Custom keeps its numbers: Orbit does not promote it." />
          </FieldLabel>
          <Select items={TIER_OPTIONS} value={tier} onValueChange={(value) => setTier(value as SendTier)}>
            <SelectTrigger id={`${ids}-tier`} className="w-48"><SelectValue /></SelectTrigger>
            <SelectContent>
              {TIER_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
            </SelectContent>
          </Select>
        </Field>
        {tier === 'custom' ? (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            {LIMIT_FIELDS.map((field) => (
              <Field key={field.name}>
                <FieldLabel htmlFor={`${ids}-${field.name}`}>{field.label}</FieldLabel>
                <Input
                  id={`${ids}-${field.name}`}
                  inputMode="numeric"
                  autoComplete="off"
                  value={limits[field.name]}
                  onChange={(event) => setLimits((current) => ({ ...current, [field.name]: event.target.value }))}
                />
              </Field>
            ))}
          </div>
        ) : (
          <ul className="m-0 flex list-none flex-col gap-1 p-0 text-[0.8125rem]">
            {limitRows(TIER_LIMITS[tier]).map((row) => (
              <li key={row.label} className="flex gap-3">
                <span className="min-w-0 flex-1 text-muted-foreground">{row.label}</span>
                <span>{row.value}</span>
              </li>
            ))}
          </ul>
        )}
        {tier === 'custom' && !valid ? <p className="m-0 text-xs text-destructive">Each number must be a whole number from 1 to 10,000,000.</p> : null}
        {save.error ? <p role="alert" className="text-sm text-destructive">{mailProblem(save.error, 'The limits were not saved.')}</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
          <Button type="submit" disabled={!valid || save.isPending}>{save.isPending ? 'Saving…' : 'Save'}</Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}
