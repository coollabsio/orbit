/** One mail domain on Settings → Mail → Domains (the web's `DomainCard`): its setup steps, Check DNS, catch-all, on/off, remove. */
import { useQueryClient } from '@tanstack/react-query'
import * as Clipboard from 'expo-clipboard'
import { router } from 'expo-router'
import * as WebBrowser from 'expo-web-browser'
import { type ReactNode, useEffect, useEffectEvent, useRef, useState } from 'react'
import { AppState, Pressable, Share, View } from 'react-native'

import { shortDate } from '../../../data/format'
import { dnsHost, firstOpenStep, type SetupStep, setupSteps, type StepKey, zoneFile, zoneOf } from '../../../data/mail-domain-setup'
import { mailDomainKeys, type MailDomain, type MailMailbox, type MailProviderView, type MailSendRouteList } from '../../../data/mail-domains-api'
import { count, dateTime, domainHealth, isSendRouteKind, mailProblem, routeLabel, routeRecordsNote, since, spfHint } from '../../../data/mail-settings'
import { useDomainWrites, useMailDomain } from '../../../hooks/mail-domains'
import { useOrbit } from '../../../orbit-context'
import { presenceColors, useTheme } from '../../../theme'
import { Button, Icon, LabelTip, Text } from '../../../ui'
import { ActionSheet, ConfirmDialog, type ConfirmRequest } from '../Dialog'
import { Badge, Note, Select, Toggle } from '../Shell'
import { SetupStepRow, StepNote } from './DomainRecords'
import { DomainRouteRow, DomainSendRouteRow } from './DomainRoute'

/** **Check DNS** reads the domain this often until the check job has run, for at most `CHECK_WAIT_MS`. */
const CHECK_POLL_MS = 2000
const CHECK_WAIT_MS = 30_000
/** Coming back to the app checks the DNS again, at most this often. */
const AUTO_CHECK_GAP_MS = 30_000
const NO_CATCH_ALL = 'none'

const openUrl = (url: string) => void WebBrowser.openBrowserAsync(url).catch(() => undefined)

export function DomainCard({ listed, mailboxes, providers, sendRoutes, defaultOpen, justAdded }: { listed: MailDomain; mailboxes: MailMailbox[]; providers: MailProviderView[]; sendRoutes: MailSendRouteList | null; defaultOpen: boolean; justAdded: boolean }) {
  const { colors } = useTheme()
  const queryClient = useQueryClient()
  const { workspaceId } = useOrbit()
  const { check, setCatchAll, setEnabled, remove, rotate } = useDomainWrites()
  const [open, setOpen] = useState(defaultOpen)
  const [menu, setMenu] = useState(false)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const [showHint, setShowHint] = useState(false)
  // the web's toasts: the last result, shown in the card
  const [notice, setNotice] = useState<{ text: string; error: boolean } | null>(null)
  const notify = (text: string, error = false) => setNotice({ text, error })
  /** While a check runs: when it was asked for, the `checked_at` before it, when to stop waiting, and whether it was an automatic one. */
  const [checking, setChecking] = useState<{ startedAt: number; checkedAt: string | null; until: number; quiet: boolean } | null>(null)
  // polls until `checked_at` changes or the wait ends; a copy cached by an earlier check does not count
  const detail = useMailDomain(checking ? listed.id : null, (data, updatedAt) =>
    checking && Date.now() < checking.until && !(data && updatedAt > checking.startedAt && data.checked_at !== checking.checkedAt) ? CHECK_POLL_MS : false,
  )
  const fresh = checking && detail.data && detail.dataUpdatedAt > checking.startedAt ? detail.data : null
  const domain = fresh ?? listed
  const waiting = checking !== null && !(fresh && fresh.checked_at !== checking.checkedAt)
  const steps = setupSteps(domain)
  const unfinished = firstOpenStep(steps)
  /** Something left for the user: a blocked step waits for the server admin, not for the DNS. */
  const actionable = steps.some((step) => step.state !== 'done' && step.state !== 'blocked')
  /** The steps the user opened or closed; the others follow the state (the first unfinished one is open). */
  const [toggled, setToggled] = useState<Partial<Record<StepKey, boolean>>>({})
  const stepOpen = (key: StepKey) => toggled[key] ?? key === unfinished

  // just added (the list may show the domain before the page knows it was added): open it
  const [wasAdded, setWasAdded] = useState(justAdded)
  if (justAdded && !wasAdded) {
    setWasAdded(true)
    setOpen(true)
  }

  useEffect(() => {
    if (!checking) return
    if (!waiting) {
      // the card shows the result at once; the list reads it again
      void queryClient.invalidateQueries({ queryKey: mailDomainKeys.domains(workspaceId), exact: true })
      return
    }
    const timer = setTimeout(
      () => {
        setChecking(null)
        if (!checking.quiet) notify('The DNS has not answered yet. Orbit checks again on its next run.')
      },
      Math.max(0, checking.until - Date.now()),
    )
    return () => clearTimeout(timer)
  }, [checking, waiting, queryClient, workspaceId])

  function checkNow(quiet = false) {
    check.mutate(domain.id, {
      onSuccess: () => setChecking({ startedAt: Date.now(), checkedAt: domain.checked_at ?? null, until: Date.now() + CHECK_WAIT_MS, quiet }),
      onError: (error) => {
        if (!quiet) notify(mailProblem(error, 'The check could not start.'), true)
      },
    })
  }

  // Back from the DNS panel (the app comes to the front): check again, at most every AUTO_CHECK_GAP_MS, while a step is not done.
  const lastAutoCheck = useRef(0)
  const autoCheck = useEffectEvent(() => {
    if (check.isPending || waiting || Date.now() - lastAutoCheck.current < AUTO_CHECK_GAP_MS) return
    lastAutoCheck.current = Date.now()
    checkNow(true)
  })
  useEffect(() => {
    if (!open || !actionable || domain.status === 'disabled') return
    const subscription = AppState.addEventListener('change', (state) => {
      if (state === 'active') autoCheck()
    })
    return () => subscription.remove()
  }, [open, actionable, domain.status])

  function removeDomain() {
    if (domain.address_count > 0) {
      setConfirm({
        title: `${domain.name} has addresses`,
        description: `${count(domain.address_count, 'address uses', 'addresses use')} this domain. Remove those mailboxes and aliases first; then the domain can be removed.`,
        confirmLabel: 'Go to mailboxes',
        onConfirm: () => router.push('/settings/mail/mailboxes'),
      })
      return
    }
    setConfirm({
      title: `Remove ${domain.name}?`,
      description: 'No address uses it. Orbit stops receiving mail for it, and you can delete its DNS records afterwards.',
      confirmLabel: 'Remove',
      danger: true,
      onConfirm: () => remove.mutate(domain.id, { onError: (error) => notify(mailProblem(error, 'The domain could not be removed.'), true) }),
    })
  }

  function rotateKey() {
    setConfirm({
      title: `Make a new DKIM key for ${domain.name}?`,
      description: 'Orbit keeps signing with the current key until the new record is published and found by the DNS check. Then it switches, and you can delete the old record a few days later.',
      confirmLabel: 'Make new key',
      onConfirm: () =>
        rotate.mutate(domain.id, {
          onSuccess: () => {
            setToggled((value) => ({ ...value, send: true }))
            notify('New DKIM key made. Publish its record under Send mail.')
          },
          onError: (error) => notify(mailProblem(error, 'The new key could not be made.'), true),
        }),
    })
  }

  function copyZoneFile() {
    Clipboard.setStringAsync(zoneFile(domain)).then(
      (done) => (done ? notify('Zone file copied. Paste it into the import of your DNS panel.') : notify('Could not copy the zone file.', true)),
      () => notify('Could not copy the zone file.', true),
    )
  }

  /** The web downloads `<domain>.zone`; a phone shares the text (to Files, mail, a note…). */
  const shareZoneFile = () => void Share.share({ message: zoneFile(domain), title: `${domain.name}.zone` }).catch(() => undefined)

  const health = domainHealth(domain)
  const zone = zoneOf(domain)
  const checked = Boolean(domain.checked_at)
  const host = dnsHost(domain.nameservers)
  const verify = domain.records.find((record) => record.kind === 'verify')
  const sendKind = domain.send_route && isSendRouteKind(domain.send_route.kind) ? domain.send_route.kind : null
  const routeNote = sendKind ? routeRecordsNote(sendKind, domain.name) : null
  const spf = sendKind && sendKind !== 'direct' ? spfHint(sendKind, null) : null
  const catchAllOptions = [
    { value: NO_CATCH_ALL, label: 'None: refuse unknown addresses' },
    ...mailboxes.map((mailbox) => ({ value: mailbox.id, label: `${mailbox.name} (${mailbox.addresses[0]?.address ?? 'no address'})` })),
  ]
  const disabled = domain.status === 'disabled'

  /** What each step shows above its records. */
  const extras: Partial<Record<StepKey, ReactNode>> = {
    receive: providers.length > 0 || domain.provider ? <DomainRouteRow domain={domain} providers={providers} notify={notify} /> : null,
    send: domain.send_route ? (
      routeNote ? <StepNote>{routeNote}</StepNote> : null
    ) : (
      <StepNote>No send route: this domain cannot send. Pick one in Send through below, or ask your server admin for a default route.</StepNote>
    ),
    inbox: spf ? (
      <StepNote>
        The SPF record includes <Text className="font-mono text-[12px] text-muted-foreground">{spf}</Text> for {routeLabel(domain.send_route?.kind ?? '')}.
      </StepNote>
    ) : null,
  }
  const blocked: Partial<Record<StepKey, ReactNode>> = {
    receive: (
      <StepNote>
        This Orbit server has no mail host name, so Orbit cannot say which MX record to publish. An instance administrator sets{' '}
        <Text className="font-mono text-[12px] text-muted-foreground">ORBIT_MAIL_HOSTNAME</Text>
        {providers.length > 0 ? ', or you can receive through a provider above.' : '.'}
      </StepNote>
    ),
    send: <StepNote>This domain has no DKIM key yet. Tap Check DNS; Orbit makes the key with the next check.</StepNote>,
  }

  return (
    <View className="rounded-lg bg-card shadow-[0_0_0_1px_var(--color-border)]">
      <View className="min-h-12 flex-row items-center gap-2 pr-3">
        <Pressable accessibilityRole="button" accessibilityState={{ expanded: open }} onPress={() => setOpen((value) => !value)} className="min-h-12 min-w-0 flex-1 flex-row items-center gap-2 py-2 pl-3 active:bg-muted/40">
          <Icon name={open ? 'chevronDown' : 'chevronRight'} size={16} color={colors.mutedForeground} />
          <View className="min-w-0 flex-1 gap-1">
            <Text numberOfLines={1} className="font-noto-medium text-[14px] text-foreground">
              {domain.name}
            </Text>
            {actionable && !disabled ? <StepDots steps={steps.filter((step) => step.state !== 'blocked')} /> : null}
          </View>
        </Pressable>
        <Pressable accessibilityRole="button" accessibilityLabel={`${health.label}: ${health.hint}`} onPress={() => setShowHint((value) => !value)}>
          <Badge label={health.label} tone={health.variant} />
        </Pressable>
        <Toggle
          label={`Receive mail for ${domain.name}`}
          value={!disabled}
          disabled={setEnabled.isPending}
          onChange={(enabled) => setEnabled.mutate({ domainId: domain.id, enabled }, { onError: (error) => notify(mailProblem(error, 'The domain could not be changed.'), true) })}
        />
      </View>
      {/* the web shows the badge's hint on hover */}
      {showHint ? <Text className="px-4 pb-2 text-[12px] leading-[17px] text-muted-foreground">{health.hint}</Text> : null}
      {open ? (
        <View className="overflow-hidden rounded-b-lg border-t border-border bg-background">
          <View className="gap-2 px-4 py-2.5">
            {host ? (
              <View className="flex-row flex-wrap items-center gap-x-2 gap-y-0.5">
                <Text className="text-[12px] text-muted-foreground">
                  DNS at <Text className="font-noto-medium text-[12px] text-foreground">{host.name}</Text>
                </Text>
                {host.url ? (
                  <Pressable accessibilityRole="link" onPress={() => openUrl(host.url ?? '')} className="flex-row items-center gap-0.5">
                    <Text className="font-noto-medium text-[12px] text-primary">Open DNS settings</Text>
                    <Icon name="arrowUpRight" size={12} color={colors.primary} />
                  </Pressable>
                ) : null}
              </View>
            ) : (
              <Text className="text-[12px] text-muted-foreground">Add these records where the DNS of {zone} is hosted.</Text>
            )}
            <View className="flex-row items-center gap-1.5">
              <Button label={waiting ? 'Checking…' : 'Check DNS'} icon="refresh" size="sm" variant={actionable ? 'primary' : 'outline'} loading={waiting} disabled={check.isPending || waiting} onPress={() => checkNow()} />
              <Button label="More" icon="more" size="sm" variant="ghost" onPress={() => setMenu(true)} />
            </View>
          </View>
          {notice ? (
            <View accessibilityLiveRegion="polite" className="border-t border-border px-4 py-2">
              <Note error={notice.error}>{notice.text}</Note>
            </View>
          ) : null}
          {domain.status === 'suspended' ? (
            <View className="border-t border-border p-3">
              <View className="gap-1 rounded-lg border border-destructive/40 bg-destructive/5 p-3">
                <View className="flex-row items-center gap-1.5">
                  <Icon name="danger" size={16} color={colors.destructive} />
                  <Text className="font-noto-medium text-[13px] text-destructive">The verification record is gone</Text>
                </View>
                <Text className="text-[12px] leading-[17px] text-destructive/90">
                  {verify?.status.status === 'wrong' ? `The TXT record at ${verify.name} has another value. ` : `The TXT record at ${verify?.name ?? `_orbit-verify.${domain.name}`} was not found. `}
                  {domain.grace_ends_at ? `Orbit receives mail for ${domain.name} until ${dateTime(domain.grace_ends_at)}, then refuses it. ` : null}
                  Publish the record again under Verify the domain, then tap Check DNS.
                </Text>
              </View>
            </View>
          ) : null}
          <View accessibilityLabel={`Setup of ${domain.name}`} className="border-t border-border">
            {steps.map((step, index) => (
              <SetupStepRow
                key={step.key}
                first={index === 0}
                step={step}
                index={index}
                open={stepOpen(step.key)}
                onToggle={() => setToggled((value) => ({ ...value, [step.key]: !stepOpen(step.key) }))}
                zone={zone}
                checked={checked}
                extra={extras[step.key]}
                blocked={step.state === 'blocked' ? blocked[step.key] : undefined}
              />
            ))}
          </View>
          <View className="flex-row items-center gap-2 border-t border-border px-4 py-2">
            <View className="min-w-0 flex-1">
              <LabelTip
                label={statusText(domain)}
                className="text-[12px] leading-[17px] text-muted-foreground"
                text="Orbit checks the DNS every 10 minutes for 72 hours after you add a domain, then less often. While this page is open, it also checks when you come back to the app. DNS changes can take a few minutes to show."
              />
            </View>
          </View>
          {sendRoutes && sendRoutes.routes.length > 0 ? <DomainSendRouteRow domain={domain} routes={sendRoutes} notify={notify} /> : null}
          <View className="gap-1.5 border-t border-border px-4 py-3">
            <LabelTip label="Catch-all" className="font-noto-medium text-[13px] text-foreground" text={`The mailbox that gets mail for addresses of ${domain.name} that no mailbox has.`} />
            <Select
              label={`Catch-all for ${domain.name}`}
              value={domain.catch_all_mailbox_id ?? NO_CATCH_ALL}
              options={catchAllOptions}
              disabled={setCatchAll.isPending}
              onChange={(value) =>
                setCatchAll.mutate(
                  { domainId: domain.id, mailboxId: value === NO_CATCH_ALL ? null : value },
                  { onSuccess: () => notify('Catch-all saved'), onError: (error) => notify(mailProblem(error, 'The catch-all could not be saved.'), true) },
                )
              }
            />
          </View>
        </View>
      ) : null}
      <ActionSheet
        visible={menu}
        title={domain.name}
        onClose={() => setMenu(false)}
        actions={[
          { label: 'Copy records as zone file', icon: 'copy', onPress: copyZoneFile },
          { label: 'Share zone file', icon: 'documentDownload', onPress: shareZoneFile },
          {
            label: domain.dkim_rotating ? 'New DKIM key waits for its record' : 'Make a new DKIM key',
            icon: 'refresh2',
            disabled: !domain.records.some((record) => record.kind === 'dkim') || domain.dkim_rotating || rotate.isPending,
            onPress: rotateKey,
          },
          { label: 'Remove domain', icon: 'trash', destructive: true, disabled: remove.isPending, onPress: removeDomain },
        ]}
      />
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </View>
  )
}

/** One dot for each step the user can do: filled when done. Shown while one is left. */
function StepDots({ steps }: { steps: SetupStep[] }) {
  const { colors } = useTheme()
  const done = steps.filter((step) => step.state === 'done').length
  return (
    <View className="flex-row items-center gap-2">
      <Text className="text-[12px] text-muted-foreground">
        {done} of {steps.length} steps
      </Text>
      <View className="flex-row gap-1">
        {steps.map((step) => (
          <View
            key={step.key}
            className={`size-1.5 rounded-full ${step.state === 'done' || step.state === 'problem' ? '' : 'bg-muted-foreground/30'}`}
            style={step.state === 'done' ? { backgroundColor: presenceColors.online } : step.state === 'problem' ? { backgroundColor: colors.destructive } : undefined}
          />
        ))}
      </View>
    </View>
  )
}

function statusText(domain: MailDomain): string {
  const checked = domain.checked_at ? `Checked ${since(domain.checked_at)}.` : 'Not checked yet.'
  switch (domain.status) {
    case 'pending':
      return checked
    case 'verified':
      return `Receives mail${domain.verified_at ? ` since ${shortDate(domain.verified_at)}` : ''}. ${count(domain.address_count, 'address', 'addresses')}. ${checked}`
    case 'suspended':
      return `Still receives mail during the grace period. ${checked}`
    case 'disabled':
      return 'Mail for this domain is refused.'
  }
}
