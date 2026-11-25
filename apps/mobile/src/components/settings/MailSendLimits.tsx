/** The web's Admin → Limits → Email (`MailSendingAdmin.tsx`): the tier limits and the sending of each workspace. */
import { useState } from 'react'
import { View } from 'react-native'

import type { SendTier } from '@orbit/api/generated/types.gen'
import { LIMIT_FIELDS, type LimitTexts, limitNumbers, limitsValid, limitTexts, type MailSendPolicy, STANDARD_LIMITS, type TierLimits } from '../../data/admin-mail-api'
import { count, limitRows, mailProblem, percent, since, TIER_LABELS } from '../../data/mail-settings'
import { useAuthOptions } from '../../hooks/account'
import { useSendLimitWrites, useSendPolicies, useSendTiers } from '../../hooks/admin-mail'
import { Button, Input, LabelTip, Text } from '../../ui'
import { ConfirmDialog, type ConfirmRequest, Dialog } from './Dialog'
import { Badge, Note, Select, SettingsCard, SettingsRow } from './Shell'

const INVALID = 'Each number must be a whole number from 1 to 10,000,000.'

/** The five numbers of a tier or of a custom policy. */
function LimitInputs({ texts, onChange }: { texts: LimitTexts; onChange: (texts: LimitTexts) => void }) {
  return (
    <View className="gap-3">
      {LIMIT_FIELDS.map((field) => (
        <Input key={field.name} label={field.label} keyboardType="number-pad" autoComplete="off" value={texts[field.name]} onChangeText={(value) => onChange({ ...texts, [field.name]: value })} />
      ))}
    </View>
  )
}

/** Tier limits, then Sending by workspace. */
export function MailSendLimits() {
  return (
    <>
      <TierDefaultsCard />
      <SendPoliciesCard />
    </>
  )
}

/** The limits of the New and Standard tiers, which every workspace on those tiers uses. */
function TierDefaultsCard() {
  const tiers = useSendTiers()
  // here, not in the form: a save remounts the form
  const save = useSendLimitWrites().tiers
  const cloud = useAuthOptions().data?.billing_enabled ?? false
  return (
    <SettingsCard
      title="Tier limits"
      description={
        cloud
          ? 'Limits and the pause apply only to mail sent through a shared route: the instance default route or Direct. A workspace\'s own relay or email API has none. A new workspace starts on New. Orbit moves it to Standard after 14 days of sending without a pause, with under 2 % hard bounces and under 0.1 % complaints. Sending pauses when, over at least 100 recipients in 7 days, more than 5 % bounce hard or more than 0.3 % complain.'
          : 'Limits and the pause apply only to mail sent through a shared route: the instance default route or Direct. A workspace\'s own relay or email API has none. A new workspace starts on Unlimited: it has no send limits. To limit a workspace, choose New, Standard or Custom under Sending by workspace. A workspace on New moves to Standard after 14 days of sending without a pause, with under 2 % hard bounces and under 0.1 % complaints. Sending pauses when, over at least 100 recipients in 7 days, more than 5 % bounce hard or more than 0.3 % complain.'
      }
    >
      {tiers.isPending ? <Note loading>Loading tier limits…</Note> : null}
      {tiers.isError ? (
        <View className="flex-row flex-wrap items-center gap-2">
          <Note error>{mailProblem(tiers.error, 'The tier limits could not be loaded.')}</Note>
          <Button label="Retry" variant="ghost" onPress={() => void tiers.refetch()} />
        </View>
      ) : null}
      {/* a fresh form for each saved version */}
      {tiers.data ? <TierForm key={JSON.stringify(tiers.data)} saved={tiers.data} save={save} /> : null}
    </SettingsCard>
  )
}

function TierForm({ saved, save }: { saved: TierLimits; save: ReturnType<typeof useSendLimitWrites>['tiers'] }) {
  const [texts, setTexts] = useState({ new: limitTexts(saved.new), standard: limitTexts(saved.standard) })
  const next: TierLimits = { new: limitNumbers(texts.new), standard: limitNumbers(texts.standard) }
  const valid = limitsValid(next.new) && limitsValid(next.standard)
  const changed = JSON.stringify(next) !== JSON.stringify(saved)

  return (
    <View className="gap-4">
      {(['new', 'standard'] as const).map((tier, index) => (
        <View key={tier} className={`gap-2 ${index > 0 ? 'border-t border-border pt-4' : ''}`}>
          <Text accessibilityRole="header" className="font-noto-semibold text-[16px]">{TIER_LABELS[tier]}</Text>
          <LimitInputs texts={texts[tier]} onChange={(changedTexts) => setTexts((current) => ({ ...current, [tier]: changedTexts }))} />
        </View>
      ))}
      {!valid ? <Note error>{INVALID}</Note> : null}
      {save.isError ? <Note error>{mailProblem(save.error, 'The tier limits could not be saved.')}</Note> : null}
      {save.isSuccess && !changed ? <Note>Tier limits saved.</Note> : null}
      <View className="flex-row">
        <Button label={save.isPending ? 'Saving…' : 'Save'} disabled={!valid || !changed || save.isPending} onPress={() => {
          save.reset()
          save.mutate(next)
        }} />
      </View>
    </View>
  )
}

/** Every workspace's tier, use, bounce and complaint rates and pause, with **Resume** and **Set limits**. */
function SendPoliciesCard() {
  const policies = useSendPolicies()
  const [editing, setEditing] = useState<MailSendPolicy | null>(null)
  return (
    <SettingsCard title="Sending by workspace" description="What each workspace sent: recipients in the last hour and day, and its rates over 7 days. Paused workspaces come first." flush>
      {policies.isPending ? <SettingsRow first><Note loading>Loading workspaces…</Note></SettingsRow> : null}
      {policies.isError ? (
        <SettingsRow first>
          <View className="flex-1 flex-row flex-wrap items-center gap-2">
            <Note error>{mailProblem(policies.error, 'The workspaces could not be loaded.')}</Note>
            <Button label="Retry" variant="ghost" onPress={() => void policies.refetch()} />
          </View>
        </SettingsRow>
      ) : null}
      {policies.data?.length === 0 ? <SettingsRow first><Note>No workspace has sent mail yet.</Note></SettingsRow> : null}
      {policies.data?.map((policy, index) => <PolicyRow key={policy.workspace_id} first={index === 0} policy={policy} onSetLimits={() => setEditing(policy)} />)}
      {editing ? <LimitsForm policy={editing} onClose={() => setEditing(null)} /> : null}
    </SettingsCard>
  )
}

function PolicyRow({ policy, first, onSetLimits }: { policy: MailSendPolicy; first: boolean; onSetLimits: () => void }) {
  const resume = useSendLimitWrites().resume
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const number = (value: number) => value.toLocaleString('en-US')
  return (
    <View className={`gap-2 border-border px-4 py-3 ${first ? '' : 'border-t'}`}>
      <View className="gap-0.5">
        <View className="flex-row flex-wrap items-center gap-2">
          <Text className="font-noto-medium text-[13px]">{policy.workspace_name}</Text>
          <Badge label={TIER_LABELS[policy.tier]} tone="outline" />
          {policy.paused_at ? <Badge label="Paused" tone="destructive" /> : null}
        </View>
        <Text className="text-[12px] leading-[17px] text-muted-foreground">
          {policy.limits
            ? `${number(policy.use_hour)} of ${number(policy.limits.per_ws_hour)} this hour · ${number(policy.use_day)} of ${number(policy.limits.per_ws_day)} today`
            : `${number(policy.use_hour)} this hour · ${number(policy.use_day)} today`}
        </Text>
        <Text className="text-[12px] leading-[17px] text-muted-foreground">
          7 days: {count(policy.recipients_7d, 'recipient', 'recipients')} · {percent(policy.hard_bounce_rate_7d)} bounced · {percent(policy.complaint_rate_7d)} complained
        </Text>
        {policy.paused_at ? (
          <Text className="text-[12px] leading-[17px] text-destructive">
            Paused {since(policy.paused_at)}{policy.pause_reason ? `: ${policy.pause_reason}` : '.'}
          </Text>
        ) : null}
        {resume.isError ? <Note error>{mailProblem(resume.error, 'Sending could not be resumed.')}</Note> : null}
        {resume.isSuccess && !policy.paused_at ? <Note>{policy.workspace_name} sends again.</Note> : null}
      </View>
      <View className="flex-row flex-wrap gap-1">
        {policy.paused_at ? (
          <Button
            label="Resume"
            variant="outline"
            size="sm"
            disabled={resume.isPending}
            onPress={() =>
              setConfirm({
                title: `Resume sending for ${policy.workspace_name}?`,
                description: 'Its held messages go out at the next run. If the rates stay high, sending pauses again.',
                confirmLabel: 'Resume',
                onConfirm: () => resume.mutate(policy.workspace_id),
              })
            }
          />
        ) : null}
        <Button label="Set limits" variant="ghost" size="sm" onPress={onSetLimits} />
      </View>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </View>
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
  const save = useSendLimitWrites().policy
  const tiers = useSendTiers()
  const [tier, setTier] = useState<SendTier>(policy.tier)
  const [texts, setTexts] = useState(() => limitTexts(policy.limits ?? STANDARD_LIMITS))
  const numbers = limitNumbers(texts)
  const valid = tier !== 'custom' || limitsValid(numbers)
  const close = () => {
    if (!save.isPending) onClose()
  }
  const submit = () => {
    if (!valid || save.isPending) return
    save.mutate({ workspaceId: policy.workspace_id, body: { tier, custom_limits: tier === 'custom' ? numbers : null } }, { onSuccess: onClose })
  }
  const tierLimits = tier === 'new' || tier === 'standard' ? tiers.data?.[tier] : null

  return (
    <Dialog
      visible
      title={`Limits of ${policy.workspace_name}`}
      onClose={close}
      footer={
        <>
          <Button label="Cancel" variant="outline" disabled={save.isPending} onPress={close} />
          <Button label={save.isPending ? 'Saving…' : 'Save'} disabled={!valid || save.isPending} onPress={submit} />
        </>
      }
    >
      <View className="gap-2">
        <LabelTip label="Tier" text="New and Standard use the tier limits above on this page. A workspace on Custom keeps its own numbers: Orbit does not promote it. Unlimited has no send limits." />
        <Select label="Tier" value={tier} options={TIER_OPTIONS} onChange={setTier} />
      </View>
      {tier === 'custom' ? <LimitInputs texts={texts} onChange={setTexts} /> : null}
      {tierLimits ? (
        <View className="gap-1">
          {limitRows(tierLimits).map((row) => (
            <View key={row.label} className="flex-row gap-3">
              <Text className="flex-1 text-[13px] text-muted-foreground">{row.label}</Text>
              <Text className="text-[13px]">{row.value}</Text>
            </View>
          ))}
        </View>
      ) : null}
      {tier === 'custom' && !valid ? <Note error>{INVALID}</Note> : null}
      {save.isError ? <Note error>{mailProblem(save.error, 'The limits were not saved.')}</Note> : null}
    </Dialog>
  )
}
