import { useEffect, useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { ChevronRight, Danger, Global, Refresh } from 'reicon-react'
import { cn } from 'cn'
import type { MailDomain, MailMailbox, MailProviderView } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { EmptyState } from '@/components/common/EmptyState'
import { InfoTip } from '@/components/common/InfoTip'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { mailKeys } from '@/features/mail/api/keys'
import { useCheckDomain, useCreateDomain, useDeleteDomain, useSetCatchAll, useSetDomainEnabled } from '@/features/mail/api/mutations'
import { useMailboxes, useMailDomain, useMailDomains, useMailProviders } from '@/features/mail/api/queries'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { shortDate } from '@/lib/format'
import { DomainRouteRow, RouteSelect } from './DomainRoute'
import { MailDomainRecords } from './MailDomainRecords'
import { count, dateTime, DIRECT, domainHealth, isMailDisabled, mailProblem, since } from './mailSettingsLib'

/** **Check now** reads the domain this often until the check job has run, for at most `CHECK_WAIT_MS`. */
const CHECK_POLL_MS = 2000
const CHECK_WAIT_MS = 30_000
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
  const create = useCreateDomain()
  const [name, setName] = useState('')
  const [route, setRoute] = useState(DIRECT)
  /** The server said the instance switch is off; it stays shown until the page is opened again. */
  const [disabled, setDisabled] = useState(false)

  function submit(event: FormEvent) {
    event.preventDefault()
    const trimmed = name.trim()
    if (!trimmed || create.isPending) return
    create.mutate({ name: trimmed, providerId: route === DIRECT ? null : route }, {
      onSuccess: (domain) => { setName(''); toast(`${domain.name} added`) },
      onError: (error) => { if (isMailDisabled(error)) setDisabled(true) },
    })
  }

  return (
    <>
      {disabled ? (
        <Alert>
          <Danger />
          <AlertTitle>Mail is off on this Orbit server</AlertTitle>
          <AlertDescription>An instance administrator can turn it on in Admin → Inbound mail.</AlertDescription>
        </Alert>
      ) : null}
      <SettingsCard title="Add a domain" description="Orbit receives mail for the domain once its verification record is in the DNS.">
        <form className="flex flex-col gap-3" onSubmit={submit}>
          <Field>
            <FieldLabel htmlFor="mail-domain-name">Domain</FieldLabel>
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
            <FieldDescription>A domain belongs to one workspace. A subdomain of a domain of another workspace is refused.</FieldDescription>
          </Field>
          {providers.length > 0 ? (
            <Field>
              <FieldLabel htmlFor="mail-domain-route">Receive mail</FieldLabel>
              <RouteSelect id="mail-domain-route" label="Where the domain's mail arrives" value={route} providers={providers} disabled={disabled} onChange={setRoute} />
              <FieldDescription>Through a provider, this server needs no open port 25. You can change it later.</FieldDescription>
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
      {domains.data?.map((domain) => <DomainCard key={domain.id} listed={domain} mailboxes={mailboxes.data ?? []} providers={providers} />)}
    </>
  )
}

function DomainCard({ listed, mailboxes, providers }: { listed: MailDomain; mailboxes: MailMailbox[]; providers: MailProviderView[] }) {
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const workspaceId = useWorkspace().workspace.id
  const check = useCheckDomain()
  const setCatchAll = useSetCatchAll()
  const setEnabled = useSetDomainEnabled()
  const remove = useDeleteDomain()
  const [open, setOpen] = useState(false)
  /** While a check runs: when it was asked for, the `checked_at` before it and when to stop waiting. */
  const [checking, setChecking] = useState<{ startedAt: number; checkedAt: string | null; until: number } | null>(null)
  // polls until `checked_at` changes or the wait ends; a copy cached by an earlier check does not count
  const detail = useMailDomain(checking ? listed.id : null, (data, updatedAt) =>
    checking && Date.now() < checking.until && !(data && updatedAt > checking.startedAt && data.checked_at !== checking.checkedAt)
      ? CHECK_POLL_MS
      : false,
  )
  const fresh = checking && detail.data && detail.dataUpdatedAt > checking.startedAt ? detail.data : null
  const domain = fresh ?? listed
  const waiting = checking !== null && !(fresh && fresh.checked_at !== checking.checkedAt)

  useEffect(() => {
    if (!checking) return
    if (!waiting) {
      // the card shows the result at once; the list reads it again
      void queryClient.invalidateQueries({ queryKey: mailKeys.domains(workspaceId), exact: true })
      return
    }
    const timer = setTimeout(() => {
      setChecking(null)
      toast('The DNS has not answered yet. Orbit checks again on its next run.')
    }, Math.max(0, checking.until - Date.now()))
    return () => clearTimeout(timer)
  }, [checking, waiting, queryClient, workspaceId])

  function checkNow() {
    check.mutate(domain.id, {
      onSuccess: () => setChecking({ startedAt: Date.now(), checkedAt: domain.checked_at ?? null, until: Date.now() + CHECK_WAIT_MS }),
      onError: (error) => toast.error(mailProblem(error, 'The check could not start.')),
    })
  }

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

  const health = domainHealth(domain)
  const verify = domain.records.find((record) => record.kind === 'verify')
  const catchAllOptions = [
    { value: NO_CATCH_ALL, label: 'None: refuse unknown addresses' },
    ...mailboxes.map((mailbox) => ({ value: mailbox.id, label: `${mailbox.name} (${mailbox.addresses[0]?.address ?? 'no address'})` })),
  ]
  const catchAll = domain.catch_all_mailbox_id ?? NO_CATCH_ALL

  return (
    <section className="m-0 flex w-full min-w-0 flex-col rounded-lg bg-card shadow-[0_0_0_1px_var(--border)]">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={`mail-domain-${domain.id}`}
        onClick={() => setOpen((value) => !value)}
        className={cn('flex min-h-12 w-full items-center gap-2 py-2 pr-3 pl-3 text-left text-sm font-medium text-foreground hover:bg-muted/40', open ? 'rounded-t-lg' : 'rounded-lg')}
      >
        <ChevronRight aria-hidden className={cn('size-4 shrink-0 text-muted-foreground transition-transform duration-[120ms]', open && 'rotate-90')} />
        <span className="min-w-0 flex-1 truncate">{domain.name}</span>
        <Badge variant={health.variant} title={health.hint}>{health.label}</Badge>
      </button>
      {open ? (
        <div id={`mail-domain-${domain.id}`} className="relative min-w-0 overflow-hidden rounded-b-lg border-t bg-background">
          <div className="flex flex-col divide-y">
            <SettingsRow className="flex-nowrap">
              <span className="min-w-0 flex-1 text-xs text-muted-foreground">{statusText(domain)}</span>
              <div className="flex shrink-0 items-center gap-2">
                <Button type="button" variant="outline" disabled={check.isPending || waiting} onClick={checkNow}>
                  <Refresh />
                  {waiting ? 'Checking…' : 'Check now'}
                </Button>
                <Button type="button" variant="ghost" className="text-destructive" disabled={remove.isPending} onClick={() => void removeDomain()}>Remove</Button>
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
                    Publish the record again, then click Check now.
                  </AlertDescription>
                </Alert>
              </div>
            ) : null}
            {providers.length > 0 || domain.provider ? <DomainRouteRow domain={domain} providers={providers} /> : null}
            <SettingsRow className="flex-nowrap">
              <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
                Mail on
                <InfoTip text={`Off: Orbit refuses all mail for ${domain.name}. Mailboxes and their mail stay.`} />
              </span>
              <Switch
                aria-label={`Receive mail for ${domain.name}`}
                checked={domain.status !== 'disabled'}
                disabled={setEnabled.isPending}
                onCheckedChange={(enabled: boolean) => setEnabled.mutate({ domainId: domain.id, enabled }, {
                  onError: (error) => toast.error(mailProblem(error, 'The domain could not be changed.')),
                })}
              />
            </SettingsRow>
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
            <div className="px-2 py-1">
              <MailDomainRecords records={domain.records} checked={Boolean(domain.checked_at)} />
            </div>
          </div>
        </div>
      ) : null}
    </section>
  )
}

function statusText(domain: MailDomain): string {
  const checked = domain.checked_at ? ` Last checked ${since(domain.checked_at)}.` : ' Not checked yet.'
  switch (domain.status) {
    case 'pending':
      return `Publish the records below at your DNS provider. Orbit checks every 10 minutes for 72 hours after you add the domain; Check now checks at once.${checked}`
    case 'verified':
      return `Receives mail${domain.verified_at ? ` since ${shortDate(domain.verified_at)}` : ''}. ${count(domain.address_count, 'address', 'addresses')}.${checked}`
    case 'suspended':
      return `Still receives mail during the grace period.${checked}`
    case 'disabled':
      return 'Mail for this domain is refused.'
  }
}
