import { useEffect, useEffectEvent, useRef, useState, type FormEvent, type ReactNode } from 'react'
import { useNavigate } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { ArrowRight, ChevronRight, Copy, Danger, DocumentDownload, Global, More, Refresh, Refresh2, Trash } from 'reicon-react'
import { cn } from 'cn'
import type { MailDomain, MailMailbox, MailProviderView, MailSendRouteList } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { EmptyState } from '@/components/common/EmptyState'
import { InfoTip } from '@/components/common/InfoTip'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { mailKeys } from '@/features/mail/api/keys'
import { useCheckDomain, useCreateDomain, useDeleteDomain, useRotateDkim, useSetCatchAll, useSetDomainEnabled } from '@/features/mail/api/mutations'
import { useMailboxes, useMailDomain, useMailDomains, useMailProviders, useSendRoutes } from '@/features/mail/api/queries'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { saveBlob } from '@/features/docs/pageExport'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { writeClipboard } from '@/lib/clipboard'
import { shortDate } from '@/lib/format'
import { DomainRouteRow, DomainSendRouteRow, RouteSelect } from './DomainRoute'
import { dnsHost, firstOpenStep, setupSteps, zoneFile, zoneOf, type SetupStep, type StepKey } from './domainSetup'
import { SetupStepRow, StepNote } from './MailDomainRecords'
import { count, dateTime, DIRECT, domainHealth, isMailDisabled, isSendRouteKind, mailProblem, routeLabel, routeRecordsNote, since, spfHint } from './mailSettingsLib'

/** **Check now** reads the domain this often until the check job has run, for at most `CHECK_WAIT_MS`. */
const CHECK_POLL_MS = 2000
const CHECK_WAIT_MS = 30_000
/** Coming back to the page checks the DNS again, at most this often. */
const AUTO_CHECK_GAP_MS = 30_000
const NO_CATCH_ALL = 'none'

/** Shown to members without `mail.manage`; the server refuses them too. */
export function MailManagersOnly({ title }: { title: string }) {
  return <SettingsCard title={title}><p className="m-0 text-muted-foreground">Only workspace owners and administrators can manage mail.</p></SettingsCard>
}

/** The workspace's mail domains: add one, publish its records, check them, catch-all, turn mail off, remove. */
export function MailDomainsPage() {
  if (!useCan('mail.manage')) return <MailManagersOnly title="Mail domains" />
  return <MailDomains />
}

function MailDomains() {
  const domains = useMailDomains()
  const mailboxes = useMailboxes()
  const providerList = useMailProviders('workspace')
  const providers = providerList.data?.providers ?? []
  const sendRoutes = useSendRoutes('workspace')
  const create = useCreateDomain()
  const [name, setName] = useState('')
  const [route, setRoute] = useState(DIRECT)
  /** The domain added last: its card opens and scrolls into view. */
  const [added, setAdded] = useState<string | null>(null)
  /** The server said the instance switch is off; it stays shown until the page is opened again. */
  const [disabled, setDisabled] = useState(false)

  function submit(event: FormEvent) {
    event.preventDefault()
    const trimmed = name.trim()
    if (!trimmed || create.isPending) return
    create.mutate({ name: trimmed, providerId: route === DIRECT ? null : route }, {
      onSuccess: (domain) => { setName(''); setAdded(domain.id); toast(`${domain.name} added. Publish its records below.`) },
      onError: (error) => { if (isMailDisabled(error)) setDisabled(true) },
    })
  }

  return (
    <>
      {disabled ? (
        <Alert>
          <Danger />
          <AlertTitle>Mail is off on this Orbit server</AlertTitle>
          <AlertDescription>An instance administrator can turn it on in Admin → Mail.</AlertDescription>
        </Alert>
      ) : null}
      <SettingsCard title="Add a domain" description="Orbit receives mail for the domain once its verification record is in the DNS.">
        <form className="flex flex-col gap-3" onSubmit={submit}>
          <Field>
            <FieldLabel htmlFor="mail-domain-name">
              Domain
              <InfoTip text="A domain belongs to one workspace. A subdomain of a domain of another workspace is refused." />
            </FieldLabel>
            <div className="flex flex-wrap items-center gap-2">
              <Input
                id="mail-domain-name"
                className="max-w-xs"
                placeholder="example.com"
                autoComplete="off"
                value={name}
                disabled={disabled}
                onChange={(event) => { setName(event.target.value); if (create.isError) create.reset() }}
              />
              <Button type="submit" disabled={!name.trim() || create.isPending || disabled}>{create.isPending ? 'Adding…' : 'Add domain'}</Button>
            </div>
          </Field>
          {providers.length > 0 ? (
            <Field>
              <FieldLabel htmlFor="mail-domain-route">
                Receive mail
                <InfoTip text="Through a provider, this server needs no open port 25. You can change it later." />
              </FieldLabel>
              <RouteSelect id="mail-domain-route" label="Where the domain's mail arrives" value={route} providers={providers} disabled={disabled} onChange={setRoute} />
            </Field>
          ) : null}
          {create.isError && !disabled ? <p role="alert" className="text-xs text-destructive">{mailProblem(create.error, 'The domain could not be added.')}</p> : null}
        </form>
      </SettingsCard>
      {domains.isPending ? <SettingsCard title="Domains"><p role="status">Loading domains…</p></SettingsCard> : null}
      {domains.isError ? (
        <SettingsCard title="Domains">
          <p role="alert">Domains could not be loaded. <Button variant="ghost" onClick={() => void domains.refetch()}>Retry</Button></p>
        </SettingsCard>
      ) : null}
      {domains.data?.length === 0 ? (
        <EmptyState icon={Global} title="No mail domains yet" description="Add a domain above, then publish its DNS records." />
      ) : null}
      {domains.data?.map((domain) => (
        <DomainCard
          key={domain.id}
          listed={domain}
          mailboxes={mailboxes.data ?? []}
          providers={providers}
          sendRoutes={sendRoutes.data ?? null}
          defaultOpen={domain.id === added || domains.data?.length === 1}
          justAdded={domain.id === added}
        />
      ))}
    </>
  )
}

function DomainCard({
  listed,
  mailboxes,
  providers,
  sendRoutes,
  defaultOpen,
  justAdded,
}: {
  listed: MailDomain
  mailboxes: MailMailbox[]
  providers: MailProviderView[]
  sendRoutes: MailSendRouteList | null
  /** Open at first. */
  defaultOpen: boolean
  /** Added a moment ago: opens and scrolls into view. */
  justAdded: boolean
}) {
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const workspaceId = useWorkspace().workspace.id
  const check = useCheckDomain()
  const setCatchAll = useSetCatchAll()
  const setEnabled = useSetDomainEnabled()
  const remove = useDeleteDomain()
  const rotate = useRotateDkim()
  const [open, setOpen] = useState(defaultOpen)
  const card = useRef<HTMLElement>(null)
  /** While a check runs: when it was asked for, the `checked_at` before it, when to stop waiting, and whether it was an automatic one. */
  const [checking, setChecking] = useState<{ startedAt: number; checkedAt: string | null; until: number; quiet: boolean } | null>(null)
  // polls until `checked_at` changes or the wait ends; a copy cached by an earlier check does not count
  const detail = useMailDomain(checking ? listed.id : null, (data, updatedAt) =>
    checking && Date.now() < checking.until && !(data && updatedAt > checking.startedAt && data.checked_at !== checking.checkedAt)
      ? CHECK_POLL_MS
      : false,
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

  // just added (the list may show the domain before the page knows it was added): open it, then scroll to it
  const [wasAdded, setWasAdded] = useState(justAdded)
  if (justAdded && !wasAdded) {
    setWasAdded(true)
    setOpen(true)
  }
  useEffect(() => {
    if (justAdded) card.current?.scrollIntoView({ block: 'start', behavior: 'smooth' })
  }, [justAdded])

  useEffect(() => {
    if (!checking) return
    if (!waiting) {
      // the card shows the result at once; the list reads it again
      void queryClient.invalidateQueries({ queryKey: mailKeys.domains(workspaceId), exact: true })
      return
    }
    const timer = setTimeout(() => {
      setChecking(null)
      if (!checking.quiet) toast('The DNS has not answered yet. Orbit checks again on its next run.')
    }, Math.max(0, checking.until - Date.now()))
    return () => clearTimeout(timer)
  }, [checking, waiting, queryClient, workspaceId])

  function checkNow(quiet = false) {
    check.mutate(domain.id, {
      onSuccess: () => setChecking({ startedAt: Date.now(), checkedAt: domain.checked_at ?? null, until: Date.now() + CHECK_WAIT_MS, quiet }),
      onError: (error) => { if (!quiet) toast.error(mailProblem(error, 'The check could not start.')) },
    })
  }

  // Back from the DNS panel: check again, at most every AUTO_CHECK_GAP_MS, while a step is not done.
  const lastAutoCheck = useRef(0)
  const autoCheck = useEffectEvent(() => {
    if (check.isPending || waiting || Date.now() - lastAutoCheck.current < AUTO_CHECK_GAP_MS) return
    lastAutoCheck.current = Date.now()
    checkNow(true)
  })
  useEffect(() => {
    if (!open || !actionable || domain.status === 'disabled') return
    const onReturn = () => { if (document.visibilityState === 'visible') autoCheck() }
    window.addEventListener('focus', onReturn)
    document.addEventListener('visibilitychange', onReturn)
    return () => {
      window.removeEventListener('focus', onReturn)
      document.removeEventListener('visibilitychange', onReturn)
    }
  }, [open, actionable, domain.status])

  async function removeDomain() {
    if (domain.address_count > 0) {
      const go = await confirmAction({
        title: `${domain.name} has addresses`,
        description: `${count(domain.address_count, 'address uses', 'addresses use')} this domain. Remove those mailboxes and aliases first; then the domain can be removed.`,
        confirmLabel: 'Go to mailboxes',
      })
      if (go) void navigate('/settings/mail/mailboxes')
      return
    }
    const confirmed = await confirmAction({
      title: `Remove ${domain.name}?`,
      description: 'No address uses it. Orbit stops receiving mail for it, and you can delete its DNS records afterwards.',
      confirmLabel: 'Remove',
      danger: true,
    })
    if (!confirmed) return
    remove.mutate(domain.id, {
      onSuccess: () => toast(`${domain.name} removed`),
      onError: (error) => toast.error(mailProblem(error, 'The domain could not be removed.')),
    })
  }

  async function rotateKey() {
    const confirmed = await confirmAction({
      title: `Make a new DKIM key for ${domain.name}?`,
      description: 'Orbit keeps signing with the current key until the new record is published and found by the DNS check. Then it switches, and you can delete the old record a few days later.',
      confirmLabel: 'Make new key',
    })
    if (!confirmed) return
    rotate.mutate(domain.id, {
      onSuccess: () => {
        setToggled((value) => ({ ...value, send: true }))
        toast('New DKIM key made. Publish its record under Send mail.')
      },
      onError: (error) => toast.error(mailProblem(error, 'The new key could not be made.')),
    })
  }

  function copyZoneFile() {
    writeClipboard(zoneFile(domain)).then(
      () => toast('Zone file copied. Paste it into the import of your DNS panel.'),
      () => toast.error('Could not copy the zone file.'),
    )
  }

  function downloadZoneFile() {
    saveBlob(new Blob([zoneFile(domain)], { type: 'text/plain' }), `${domain.name}.zone`)
  }

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
  const catchAll = domain.catch_all_mailbox_id ?? NO_CATCH_ALL

  /** What each step shows above its records. */
  const extras: Partial<Record<StepKey, ReactNode>> = {
    receive: providers.length > 0 || domain.provider ? <DomainRouteRow domain={domain} providers={providers} /> : null,
    send: domain.send_route
      ? routeNote ? <StepNote>{routeNote}</StepNote> : null
      : <StepNote>No send route: this domain cannot send. Pick one in Send through below, or ask your server admin for a default route.</StepNote>,
    inbox: spf ? <StepNote>The SPF record includes <code className="font-mono">{spf}</code> for {routeLabel(domain.send_route?.kind ?? '')}.</StepNote> : null,
  }
  const blocked: Partial<Record<StepKey, ReactNode>> = {
    receive: (
      <StepNote>
        This Orbit server has no mail host name, so Orbit cannot say which MX record to publish. An instance administrator sets <code className="font-mono">ORBIT_MAIL_HOSTNAME</code>
        {providers.length > 0 ? ', or you can receive through a provider above.' : '.'}
      </StepNote>
    ),
    send: <StepNote>This domain has no DKIM key yet. Click Check DNS; Orbit makes the key with the next check.</StepNote>,
  }

  return (
    <section ref={card} className="m-0 flex w-full min-w-0 scroll-mt-4 flex-col rounded-lg bg-card shadow-[0_0_0_1px_var(--border)]">
      <div className={cn('flex min-h-12 w-full items-center gap-3 pr-3 hover:bg-muted/40', open ? 'rounded-t-lg' : 'rounded-lg')}>
        <button
          type="button"
          aria-expanded={open}
          aria-controls={`mail-domain-${domain.id}`}
          onClick={() => setOpen((value) => !value)}
          className="flex min-h-12 min-w-0 flex-1 items-center gap-2 py-2 pl-3 text-left text-sm font-medium text-foreground"
        >
          <ChevronRight aria-hidden className={cn('size-4 shrink-0 text-muted-foreground transition-transform duration-[120ms]', open && 'rotate-90')} />
          <span className="min-w-0 flex-1 truncate">{domain.name}</span>
          {actionable && domain.status !== 'disabled' ? <StepDots steps={steps.filter((step) => step.state !== 'blocked')} /> : null}
          <Badge variant={health.variant} title={health.hint}>{health.label}</Badge>
        </button>
        <Switch
          aria-label={`Receive mail for ${domain.name}`}
          title={domain.status === 'disabled' ? 'Mail off: Orbit refuses all mail for this domain' : 'Mail on'}
          checked={domain.status !== 'disabled'}
          disabled={setEnabled.isPending}
          onCheckedChange={(enabled: boolean) => setEnabled.mutate({ domainId: domain.id, enabled }, {
            onError: (error) => toast.error(mailProblem(error, 'The domain could not be changed.')),
          })}
        />
      </div>
      {open ? (
        <div id={`mail-domain-${domain.id}`} className="relative min-w-0 overflow-hidden rounded-b-lg border-t bg-background">
          <div className="flex flex-col divide-y">
            <SettingsRow className="flex-nowrap py-2.5">
              <span className="flex min-w-0 flex-1 flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-muted-foreground">
                {host ? (
                  <>
                    <span>DNS at <span className="font-medium text-foreground">{host.name}</span></span>
                    {host.url ? (
                      <a href={host.url} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-0.5 font-medium text-primary underline-offset-2 hover:underline">
                        Open DNS settings
                        <ArrowRight aria-hidden className="size-3 -rotate-45" />
                      </a>
                    ) : null}
                  </>
                ) : (
                  <span>Add these records where the DNS of {zone} is hosted.</span>
                )}
              </span>
              <div className="flex shrink-0 items-center gap-1.5">
                <Button type="button" variant={actionable ? 'default' : 'outline'} disabled={check.isPending || waiting} onClick={() => checkNow()}>
                  <Refresh className={cn(waiting && 'animate-spin')} />
                  {waiting ? 'Checking…' : 'Check DNS'}
                </Button>
                <DropdownMenu>
                  <DropdownMenuTrigger render={<Button type="button" variant="ghost" size="icon" aria-label={`More for ${domain.name}`} />}>
                    <More />
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="w-auto min-w-56">
                    <DropdownMenuItem onClick={copyZoneFile}><Copy /> Copy records as zone file</DropdownMenuItem>
                    <DropdownMenuItem onClick={downloadZoneFile}><DocumentDownload /> Download zone file</DropdownMenuItem>
                    <DropdownMenuItem disabled={!domain.records.some((record) => record.kind === 'dkim') || domain.dkim_rotating || rotate.isPending} onClick={() => void rotateKey()}>
                      <Refresh2 /> {domain.dkim_rotating ? 'New DKIM key waits for its record' : 'Make a new DKIM key'}
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem variant="destructive" disabled={remove.isPending} onClick={() => void removeDomain()}><Trash /> Remove domain</DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            </SettingsRow>
            {domain.status === 'suspended' ? (
              <div className="p-3">
                <Alert variant="destructive">
                  <Danger />
                  <AlertTitle>The verification record is gone</AlertTitle>
                  <AlertDescription>
                    {verify?.status.status === 'wrong' ? `The TXT record at ${verify.name} has another value. ` : `The TXT record at ${verify?.name ?? `_orbit-verify.${domain.name}`} was not found. `}
                    {domain.grace_ends_at ? `Orbit receives mail for ${domain.name} until ${dateTime(domain.grace_ends_at)}, then refuses it. ` : null}
                    Publish the record again under Verify the domain, then click Check DNS.
                  </AlertDescription>
                </Alert>
              </div>
            ) : null}
            <ol aria-label={`Setup of ${domain.name}`} className="m-0 flex list-none flex-col divide-y p-0">
              {steps.map((step, index) => (
                <SetupStepRow
                  key={step.key}
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
            </ol>
            <SettingsRow className="flex-nowrap py-2">
              <span className="flex min-w-0 flex-1 items-center gap-1.5 text-xs text-muted-foreground">
                {statusText(domain)}
                <InfoTip text={`Orbit checks the DNS every 10 minutes for 72 hours after you add a domain, then less often. While this page is open, it also checks when you come back to it. DNS changes can take a few minutes to show.`} />
              </span>
              <Button type="button" variant="ghost" size="sm" className="shrink-0 text-muted-foreground" onClick={downloadZoneFile}>
                <DocumentDownload />
                Download zone file
              </Button>
            </SettingsRow>
            {sendRoutes && sendRoutes.routes.length > 0 ? <DomainSendRouteRow domain={domain} routes={sendRoutes} /> : null}
            <SettingsRow className="flex-nowrap">
              <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
                Catch-all
                <InfoTip text={`The mailbox that gets mail for addresses of ${domain.name} that no mailbox has.`} />
              </span>
              <Select
                items={catchAllOptions}
                value={catchAll}
                disabled={setCatchAll.isPending}
                onValueChange={(value) => setCatchAll.mutate({ domainId: domain.id, mailboxId: value === NO_CATCH_ALL ? null : (value as string) }, {
                  onSuccess: () => toast('Catch-all saved'),
                  onError: (error) => toast.error(mailProblem(error, 'The catch-all could not be saved.')),
                })}
              >
                <SelectTrigger aria-label={`Catch-all for ${domain.name}`} className="w-80 max-w-[55vw]"><SelectValue /></SelectTrigger>
                <SelectContent>
                  {catchAllOptions.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
                </SelectContent>
              </Select>
            </SettingsRow>
          </div>
        </div>
      ) : null}
    </section>
  )
}

/** One dot for each step the user can do: filled when done. Shown while one is left. */
function StepDots({ steps }: { steps: SetupStep[] }) {
  const done = steps.filter((step) => step.state === 'done').length
  return (
    <span className="flex items-center gap-2 text-xs font-normal text-muted-foreground">
      <span className="tabular-nums">{done} of {steps.length} steps</span>
      <span aria-hidden className="flex gap-1">
        {steps.map((step) => (
          <span
            key={step.key}
            className={cn(
              'size-1.5 rounded-full',
              step.state === 'done' ? 'bg-emerald-600 dark:bg-emerald-400' : step.state === 'problem' ? 'bg-destructive' : 'bg-muted-foreground/30',
            )}
          />
        ))}
      </span>
    </span>
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
