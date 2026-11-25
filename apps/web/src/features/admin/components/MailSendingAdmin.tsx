import { useId, useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import type { Limits, MailSendPolicy, SendTier, TierLimits } from '@orbit/api/generated/types.gen'
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
import { useResumeSending, useSetSendPolicy, useSetSendTiers } from '@/features/mail/api/mutations'
import { useSendPolicies, useSendTiers } from '@/features/mail/api/queries'
import { useBillingEnabled } from '@/features/workspaces/billing'
import { count, limitRows, mailProblem, percent, since, TIER_LABELS } from '@/features/settings/pages/mail/mailSettingsLib'

const LIMIT_FIELDS: { name: keyof Limits; label: string }[] = [
  { name: 'per_user_hour', label: 'Each member, an hour' },
  { name: 'per_user_day', label: 'Each member, a day' },
  { name: 'per_ws_hour', label: 'Workspace, an hour' },
  { name: 'per_ws_day', label: 'Workspace, a day' },
  { name: 'max_recipients', label: 'Recipients of one message' },
]

const MAX_LIMIT = 10_000_000

type LimitTexts = Record<keyof Limits, string>

const limitTexts = (limits: Limits): LimitTexts => ({
  per_user_hour: String(limits.per_user_hour),
  per_user_day: String(limits.per_user_day),
  per_ws_hour: String(limits.per_ws_hour),
  per_ws_day: String(limits.per_ws_day),
  max_recipients: String(limits.max_recipients),
})

const limitNumbers = (texts: LimitTexts) => Object.fromEntries(LIMIT_FIELDS.map((field) => [field.name, Number(texts[field.name])])) as unknown as Limits

/** Each number a whole number from 1 to 10,000,000, as the server asks. */
/** The numbers a custom policy starts from when the workspace has no limits (the Standard defaults). */
const STANDARD_LIMITS: Limits = { per_user_hour: 500, per_user_day: 2_000, per_ws_hour: 2_000, per_ws_day: 10_000, max_recipients: 100 }

const limitsValid = (limits: Limits) => LIMIT_FIELDS.every((field) => Number.isInteger(limits[field.name]) && limits[field.name] >= 1 && limits[field.name] <= MAX_LIMIT)

/** The five numbers of a tier or of a custom policy. */
function LimitInputs({ id, texts, onChange }: { id: string; texts: LimitTexts; onChange: (texts: LimitTexts) => void }) {
  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {LIMIT_FIELDS.map((field) => (
        <Field key={field.name}>
          <FieldLabel htmlFor={`${id}-${field.name}`}>{field.label}</FieldLabel>
          <Input
            id={`${id}-${field.name}`}
            inputMode="numeric"
            autoComplete="off"
            value={texts[field.name]}
            onChange={(event) => onChange({ ...texts, [field.name]: event.target.value })}
          />
        </Field>
      ))}
    </div>
  )
}

/** The limits of the New and Standard tiers, which every workspace on those tiers uses. */
export function TierDefaultsCard() {
  const tiers = useSendTiers()
  const cloud = useBillingEnabled()
  return (
    <SettingsCard
      title="Tier limits"
      description={
        cloud
          ? 'Limits and the pause apply only to mail sent through a shared route: the instance default route or Direct. A workspace\'s own relay or email API has none. A new workspace starts on New. Orbit moves it to Standard after 14 days of sending without a pause, with under 2 % hard bounces and under 0.1 % complaints. Sending pauses when, over at least 100 recipients in 7 days, more than 5 % bounce hard or more than 0.3 % complain.'
          : 'Limits and the pause apply only to mail sent through a shared route: the instance default route or Direct. A workspace\'s own relay or email API has none. A new workspace starts on Unlimited: it has no send limits. To limit a workspace, choose New, Standard or Custom under Sending by workspace. A workspace on New moves to Standard after 14 days of sending without a pause, with under 2 % hard bounces and under 0.1 % complaints. Sending pauses when, over at least 100 recipients in 7 days, more than 5 % bounce hard or more than 0.3 % complain.'
      }
    >
      {tiers.isPending ? <p role="status" className="m-0 text-sm text-muted-foreground">Loading tier limits…</p> : null}
      {tiers.isError ? (
        <p role="alert" className="m-0 text-sm text-destructive">
          {mailProblem(tiers.error, 'The tier limits could not be loaded.')}{' '}
          <Button variant="ghost" onClick={() => void tiers.refetch()}>Retry</Button>
        </p>
      ) : null}
      {/* a fresh form for each saved version */}
      {tiers.data ? <TierForm key={JSON.stringify(tiers.data)} saved={tiers.data} /> : null}
    </SettingsCard>
  )
}

function TierForm({ saved }: { saved: TierLimits }) {
  const ids = useId()
  const save = useSetSendTiers()
  const [texts, setTexts] = useState({ new: limitTexts(saved.new), standard: limitTexts(saved.standard) })
  const next: TierLimits = { new: limitNumbers(texts.new), standard: limitNumbers(texts.standard) }
  const valid = limitsValid(next.new) && limitsValid(next.standard)
  const changed = JSON.stringify(next) !== JSON.stringify(saved)

  function submit(event: FormEvent) {
    event.preventDefault()
    if (!valid || !changed || save.isPending) return
    save.mutate(next, {
      onSuccess: () => toast('Tier limits saved'),
      onError: (error) => toast.error(mailProblem(error, 'The tier limits could not be saved.')),
    })
  }

  return (
    <form className="flex flex-col gap-4" onSubmit={submit}>
      {(['new', 'standard'] as const).map((tier) => (
        <fieldset key={tier} className="m-0 flex flex-col gap-2 border-0 p-0 not-first:border-t not-first:pt-4">
          <legend className="float-left mb-1 w-full p-0 text-base font-semibold text-foreground">{TIER_LABELS[tier]}</legend>
          <LimitInputs id={`${ids}-${tier}`} texts={texts[tier]} onChange={(changedTexts) => setTexts((current) => ({ ...current, [tier]: changedTexts }))} />
        </fieldset>
      ))}
      {!valid ? <p className="m-0 text-xs text-destructive">Each number must be a whole number from 1 to 10,000,000.</p> : null}
      <div>
        <Button type="submit" disabled={!valid || !changed || save.isPending}>{save.isPending ? 'Saving…' : 'Save'}</Button>
      </div>
    </form>
  )
}

/** Every workspace's tier, use, bounce and complaint rates and pause, with **Resume** and **Set limits**. */
export function SendPoliciesCard() {
  const policies = useSendPolicies()
  const [editing, setEditing] = useState<MailSendPolicy | null>(null)
  return (
    <>
      <SettingsCard title="Sending by workspace" description="What each workspace sent: recipients in the last hour and day, and its rates over 7 days. Paused workspaces come first." flush>
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
          {policy.limits
            ? `${policy.use_hour.toLocaleString('en-US')} of ${policy.limits.per_ws_hour.toLocaleString('en-US')} this hour · ${policy.use_day.toLocaleString('en-US')} of ${policy.limits.per_ws_day.toLocaleString('en-US')} today`
            : `${policy.use_hour.toLocaleString('en-US')} this hour · ${policy.use_day.toLocaleString('en-US')} today`}
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
  { value: 'unlimited', label: 'Unlimited' },
]

/** **Set limits**: a tier, or custom numbers (1 to 10,000,000 each). */
function LimitsForm({ policy, onClose }: { policy: MailSendPolicy; onClose: () => void }) {
  const ids = useId()
  const save = useSetSendPolicy()
  const tiers = useSendTiers()
  const [tier, setTier] = useState<SendTier>(policy.tier)
  const [texts, setTexts] = useState(() => limitTexts(policy.limits ?? STANDARD_LIMITS))
  const numbers = limitNumbers(texts)
  const valid = tier !== 'custom' || limitsValid(numbers)

  function submit(event: FormEvent) {
    event.preventDefault()
    if (!valid || save.isPending) return
    save.mutate(
      { workspaceId: policy.workspace_id, body: { tier, custom_limits: tier === 'custom' ? numbers : null } },
      { onSuccess: () => { toast(`Limits of ${policy.workspace_name} saved`); onClose() } },
    )
  }

  const tierLimits = tier === 'new' || tier === 'standard' ? tiers.data?.[tier] : null
  return (
    <Modal title={`Limits of ${policy.workspace_name}`} onClose={onClose} dismissible={!save.isPending}>
      <form className="grid gap-4" onSubmit={submit}>
        <Field>
          <FieldLabel htmlFor={`${ids}-tier`}>
            Tier
            <InfoTip text="New and Standard use the tier limits above on this page. A workspace on Custom keeps its own numbers: Orbit does not promote it. Unlimited has no send limits." />
          </FieldLabel>
          <Select items={TIER_OPTIONS} value={tier} onValueChange={(value) => setTier(value as SendTier)}>
            <SelectTrigger id={`${ids}-tier`} className="w-48"><SelectValue /></SelectTrigger>
            <SelectContent>
              {TIER_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
            </SelectContent>
          </Select>
        </Field>
        {tier === 'custom' ? <LimitInputs id={`${ids}-custom`} texts={texts} onChange={setTexts} /> : null}
        {tierLimits ? (
          <ul className="m-0 flex list-none flex-col gap-1 p-0 text-[0.8125rem]">
            {limitRows(tierLimits).map((row) => (
              <li key={row.label} className="flex gap-3">
                <span className="min-w-0 flex-1 text-muted-foreground">{row.label}</span>
                <span>{row.value}</span>
              </li>
            ))}
          </ul>
        ) : null}
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
