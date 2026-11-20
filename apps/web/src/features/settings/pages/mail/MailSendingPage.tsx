import { useId, useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import { CloseCircle, MinusCircle, PauseCircle, TickCircle } from 'reicon-react'
import type { MailSendPolicy, MailSendRouteTest, MailSendRouteView } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { InfoTip } from '@/components/common/InfoTip'
import { Modal } from '@/components/common/Modal'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useDeleteSendRoute, useSetSendDefault, useTestSendRoute } from '@/features/mail/api/mutations'
import { useSendPolicy, useSendRoutes, type ProviderScope } from '@/features/mail/api/queries'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { useBillingEnabled } from '@/features/workspaces/billing'
import { useCan } from '@/features/workspaces/permissions'
import { MailManagersOnly } from './MailDomainsPage'
import { count, dateTime, defaultMaxBytes, isSendRouteKind, limitRows, mailProblem, megabytes, percent, routeLabel, since, spfHint, TIER_LABELS } from './mailSettingsLib'
import { RouteSetup, SendRouteForm } from './SendRouteForm'

/** Settings → Mail → Sending: the workspace's tier and limits, and a paused banner. Its routes are in Providers. */
export function MailSendingPage() {
  if (!useCan('mail.manage')) return <MailManagersOnly title="Sending" />
  return (
    <>
      <PausedAlert />
      <SendLimitsCard />
    </>
  )
}

function PausedAlert() {
  const policy = useSendPolicy()
  if (!policy.data?.paused_at) return null
  return (
    <Alert variant="destructive">
      <PauseCircle />
      <AlertTitle>Sending is paused for this workspace</AlertTitle>
      <AlertDescription>
        {policy.data.pause_reason ? `${policy.data.pause_reason} ` : null}
        Paused {since(policy.data.paused_at)}. New messages wait in the Outbox until the server admin resumes sending.
      </AlertDescription>
    </Alert>
  )
}

function SendLimitsCard() {
  const policy = useSendPolicy()
  const cloud = useBillingEnabled()
  return (
    <SettingsCard
      title="Limits"
      description={
        cloud
          ? 'Recipients are counted, not messages. A new workspace starts on New and moves to Standard after two weeks of good sending.'
          : 'Recipients are counted, not messages. Your server admin sets the limits of each workspace.'
      }
      flush
    >
      {policy.isPending ? <p role="status" className="m-0 p-4 text-sm text-muted-foreground">Loading limits…</p> : null}
      {policy.isError ? <p role="alert" className="m-0 p-4 text-sm text-destructive">{mailProblem(policy.error, 'The limits could not be loaded.')}</p> : null}
      {policy.data ? <PolicyRows policy={policy.data} /> : null}
    </SettingsCard>
  )
}

/** A workspace's tier, limits, use and rates (read only). */
export function PolicyRows({ policy }: { policy: MailSendPolicy }) {
  return (
    <div className="flex flex-col divide-y">
      <SettingsRow className="flex-nowrap">
        <span className="min-w-0 flex-1 text-[0.8125rem] font-medium text-foreground">Tier</span>
        <Badge variant="outline">{TIER_LABELS[policy.tier]}</Badge>
      </SettingsRow>
      {limitRows(policy.limits).map((row) => (
        <SettingsRow key={row.label} className="flex-nowrap">
          <span className="min-w-0 flex-1 text-[0.8125rem] font-medium text-foreground">{row.label}</span>
          <span className="text-[0.8125rem] text-muted-foreground">{row.value}</span>
        </SettingsRow>
      ))}
      <SettingsRow className="flex-nowrap">
        <span className="min-w-0 flex-1 text-[0.8125rem] font-medium text-foreground">Sent</span>
        <span className="text-[0.8125rem] text-muted-foreground">
          {policy.use_hour.toLocaleString('en-US')} in the last hour, {policy.use_day.toLocaleString('en-US')} today
        </span>
      </SettingsRow>
      <SettingsRow className="flex-nowrap">
        <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
          Last 7 days
          <InfoTip text="Sending pauses when, over at least 100 recipients in 7 days, more than 5 % bounce hard or more than 0.3 % complain." />
        </span>
        <span className="text-[0.8125rem] text-muted-foreground">
          {count(policy.recipients_7d, 'recipient', 'recipients')} · {percent(policy.hard_bounce_rate_7d)} bounced · {percent(policy.complaint_rate_7d)} complained
        </span>
      </SettingsRow>
    </div>
  )
}

/**
 * The outgoing routes of `scope`, a section of the Providers card. A workspace lists the server's routes too, read
 * only. Each own route has its setup, **Send test**, **Change** and **Remove**. `hostname` (the root user knows it)
 * fills the SPF of a direct route.
 */
export function OutgoingRoutes({ scope, hostname, adding, onAddClose }: { scope: ProviderScope; hostname?: string | null; adding: boolean; onAddClose: () => void }) {
  const routes = useSendRoutes(scope)
  const [editing, setEditing] = useState<MailSendRouteView | null>(null)
  const [created, setCreated] = useState<MailSendRouteView | null>(null)
  const directAvailable = routes.data?.direct_available ?? false

  return (
    <>
      {routes.isPending ? <p role="status" className="m-0 p-4 text-sm text-muted-foreground">Loading routes…</p> : null}
      {routes.isError ? (
        <p role="alert" className="m-0 p-4 text-sm text-destructive">
          {mailProblem(routes.error, 'The routes could not be loaded.')}{' '}
          <Button variant="ghost" onClick={() => void routes.refetch()}>Retry</Button>
        </p>
      ) : null}
      {routes.data?.routes.length === 0 ? (
        <p className="m-0 px-4 py-3 text-sm text-muted-foreground">
          {scope === 'instance' ? 'No outgoing providers. Without one, no workspace can send mail.' : 'No outgoing providers. Your server admin has not set one up either, so your domains cannot send yet.'}
        </p>
      ) : null}
      {routes.data?.routes.map((route) => (
        <SendRouteRow key={route.id} scope={scope} route={route} hostname={hostname} onEdit={() => setEditing(route)} />
      ))}
      {adding ? <SendRouteForm scope={scope} directAvailable={directAvailable} onClose={onAddClose} onCreated={setCreated} /> : null}
      {editing ? <SendRouteForm scope={scope} route={editing} directAvailable={directAvailable} onClose={() => setEditing(null)} /> : null}
      {created ? <RouteCreatedModal created={created} onClose={() => setCreated(null)} /> : null}
    </>
  )
}

/** After a route is added: what to set up at the provider, with the secrets the server made (shown once). */
export function RouteCreatedModal({ created, onClose }: { created: MailSendRouteView; onClose: () => void }) {
  return (
    <Modal title={`Set up ${created.name}`} description={`Do these steps for ${routeLabel(created.kind)}.`} onClose={onClose}>
      <RouteSetup route={created} />
    </Modal>
  )
}

function SendRouteRow({ scope, route, hostname, onEdit }: { scope: ProviderScope; route: MailSendRouteView; hostname?: string | null; onEdit: () => void }) {
  const remove = useDeleteSendRoute(scope)
  const setDefault = useSetSendDefault()
  const [open, setOpen] = useState<'setup' | 'test' | null>(null)
  // a server route listed to a workspace: its name and kind only
  const own = route.settings !== null && route.settings !== undefined
  const kind = isSendRouteKind(route.kind) ? route.kind : null
  const spf = kind ? spfHint(kind, route.settings, hostname) : null
  const maxBytes = route.settings?.max_message_bytes || (kind ? defaultMaxBytes(kind) : 0)

  async function removeRoute() {
    const confirmed = await confirmAction({
      title: `Remove ${route.name}?`,
      description: 'Orbit stops sending through it. A route that a domain uses, or the default route, cannot be removed.',
      confirmLabel: 'Remove',
      danger: true,
    })
    if (!confirmed) return
    remove.mutate(route.id, {
      onSuccess: () => toast(`${route.name} removed`),
      onError: (error) => toast.error(mailProblem(error, 'The route could not be removed.')),
    })
  }

  // only the server's own routes can be the default, and only Admin → Mail sets it
  const canSetDefault = own && scope === 'instance' && route.scope === 'instance'
  function saveDefault(routeId: string | null) {
    setDefault.mutate(routeId, {
      onSuccess: () => toast(routeId ? `${route.name} is the default route` : 'No default route'),
      onError: (error) => toast.error(mailProblem(error, 'The default route could not be saved.')),
    })
  }

  const toggle = (panel: 'setup' | 'test') => setOpen((current) => (current === panel ? null : panel))
  return (
    <div className="flex flex-col gap-3 px-4 py-3">
      <div className="flex flex-wrap items-start gap-3">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[0.8125rem] font-medium text-foreground">{route.name}</span>
            <Badge variant="outline">{routeLabel(route.kind)}</Badge>
            {scope === 'workspace' && route.scope === 'instance' ? <Badge variant="secondary">Server</Badge> : null}
            {route.is_default ? <Badge variant="secondary">Default</Badge> : null}
          </div>
          {own ? (
            <span className="text-xs text-muted-foreground">
              {count(route.domain_count, 'domain', 'domains')}
              {maxBytes > 0 ? ` · Up to ${megabytes(maxBytes)}` : null}
              {route.last_sent_at ? ` · Last sent ${since(route.last_sent_at)}` : ' · Nothing sent yet'}
            </span>
          ) : null}
          {own && spf ? <span className="text-xs text-muted-foreground">SPF: <code className="font-mono">{spf}</code></span> : null}
          {own && kind && !spf ? <span className="text-xs text-muted-foreground">SPF: set the relay’s SPF entry on the route.</span> : null}
          {route.last_error ? <span role="status" className="text-xs text-destructive">Last error: {route.last_error}</span> : null}
        </div>
        {own ? (
          <div className="flex flex-wrap items-center gap-1">
            <Button type="button" variant="outline" size="sm" aria-expanded={open === 'test'} onClick={() => toggle('test')}>Send test</Button>
            <Button type="button" variant="ghost" size="sm" aria-expanded={open === 'setup'} onClick={() => toggle('setup')}>{open === 'setup' ? 'Hide setup' : 'Setup'}</Button>
            {canSetDefault ? (
              <Button type="button" variant="ghost" size="sm" disabled={setDefault.isPending} onClick={() => saveDefault(route.is_default ? null : route.id)}>
                {route.is_default ? 'Unset default' : 'Set as default'}
              </Button>
            ) : null}
            <Button type="button" variant="ghost" size="sm" onClick={onEdit}>Change</Button>
            <Button type="button" variant="ghost" size="sm" className="text-destructive" disabled={remove.isPending} onClick={() => void removeRoute()}>Remove</Button>
          </div>
        ) : null}
      </div>
      {open === 'setup' ? <RouteSetup route={route} /> : null}
      {open === 'test' ? <SendTest scope={scope} route={route} /> : null}
    </div>
  )
}

const STEP_ICONS = { ok: TickCircle, failed: CloseCircle, skipped: MinusCircle } as const
const STEP_WORDS = { ok: 'OK', failed: 'Failed', skipped: 'Skipped' } as const

/** **Send test**: a message to an address the admin types, and the result of each step. */
function SendTest({ scope, route }: { scope: ProviderScope; route: MailSendRouteView }) {
  const id = useId()
  const test = useTestSendRoute(scope)
  const [to, setTo] = useState('')
  const [result, setResult] = useState<MailSendRouteTest | null>(null)

  function submit(event: FormEvent) {
    event.preventDefault()
    const address = to.trim()
    if (!address || test.isPending) return
    setResult(null)
    test.mutate({ routeId: route.id, to: address }, {
      onSuccess: setResult,
      onError: (error) => toast.error(mailProblem(error, 'The test could not run.')),
    })
  }

  return (
    <div className="flex flex-col gap-3 rounded-md border bg-muted/30 p-3">
      <form className="flex flex-wrap items-center gap-2" onSubmit={submit}>
        <label htmlFor={`${id}-to`} className="sr-only">Send a test message to</label>
        <Input id={`${id}-to`} type="email" className="max-w-xs" placeholder="you@example.com" autoComplete="email" value={to} onChange={(event) => setTo(event.target.value)} />
        <Button type="submit" variant="outline" disabled={!to.trim() || test.isPending}>{test.isPending ? 'Sending…' : 'Send test'}</Button>
      </form>
      {result ? (
        <ul aria-label={`Test of ${route.name}`} className="m-0 flex list-none flex-col gap-1.5 p-0">
          {result.steps.map((step) => {
            const Icon = STEP_ICONS[step.status]
            return (
              <li key={step.name} data-state={step.status} className="group flex items-start gap-2 text-[0.8125rem]">
                <Icon
                  aria-hidden
                  className="mt-0.5 size-4 shrink-0 text-muted-foreground group-data-[state=failed]:text-destructive group-data-[state=ok]:text-emerald-600 dark:group-data-[state=ok]:text-emerald-400"
                />
                <div className="flex min-w-0 flex-col">
                  <span className="font-medium">
                    {step.name}
                    <span className="sr-only">: {STEP_WORDS[step.status]}</span>
                  </span>
                  {step.detail ? <span className="text-xs break-words text-muted-foreground">{step.detail}</span> : null}
                </div>
              </li>
            )
          })}
        </ul>
      ) : null}
      {route.last_sent_at ? <span className="text-xs text-muted-foreground">Last message sent {dateTime(route.last_sent_at)}.</span> : null}
    </div>
  )
}
