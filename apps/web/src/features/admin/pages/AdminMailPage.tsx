import type { ReactNode } from 'react'
import { toast } from 'sonner'
import { CloseCircle, InfoCircle, TickCircle, Warning } from 'reicon-react'
import type { MailAdmin, PtrStatus } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { InfoTip } from '@/components/common/InfoTip'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Switch } from '@/components/ui/switch'
import { useSetAdminMail } from '@/features/mail/api/mutations'
import { useAdminMail, useMailProviders } from '@/features/mail/api/queries'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { dateTime, mailProblem } from '@/features/settings/pages/mail/mailSettingsLib'
import { MailTestCard } from '@/features/admin/components/MailTestCard'
import { MailConnections } from '@/features/settings/pages/mail/MailProvidersPage'

type HealthState = 'ok' | 'warning' | 'error' | 'info'

interface HealthItem {
  label: string
  state: HealthState
  detail: ReactNode
  /** What to do about it; shown when the item is not OK. */
  fix?: ReactNode
}

const STATE_ICONS: Record<HealthState, typeof TickCircle> = { ok: TickCircle, warning: Warning, error: CloseCircle, info: InfoCircle }
const STATE_ICON_COLORS = 'text-muted-foreground group-data-[state=error]:text-destructive group-data-[state=ok]:text-emerald-600 group-data-[state=warning]:text-amber-600 dark:group-data-[state=ok]:text-emerald-400 dark:group-data-[state=warning]:text-amber-400'
const STATE_WORDS: Record<HealthState, string> = { ok: 'OK', warning: 'Warning', error: 'Problem', info: 'Note' }

/**
 * The instance's mail: the switches with the health button, receiving (providers, a test message) and sending (routes
 * with the default route). The send limits are on the Limits page.
 */
export function AdminMailPage() {
  const admin = useAdminMail()
  const providers = useMailProviders('instance')
  const hasProviders = (providers.data?.providers.length ?? 0) > 0
  if (admin.isPending) return <SettingsCard title="Mail"><p role="status">Loading mail settings…</p></SettingsCard>
  if (admin.isError) {
    return <SettingsCard title="Mail"><p role="alert">{mailProblem(admin.error, 'Mail settings could not be loaded.')} <Button variant="ghost" onClick={() => void admin.refetch()}>Retry</Button></p></SettingsCard>
  }
  return (
    <>
      <DomainsCard
        admin={admin.data}
        health={<HealthButton admin={admin.data} hasProviders={hasProviders} refreshing={admin.isFetching} onRefresh={() => void admin.refetch()} />}
      />
      <MailConnections scope="instance" hostname={admin.data.hostname} />
      <MailTestCard admin={admin.data} hasProviders={hasProviders} />
    </>
  )
}

function DomainsCard({ admin, health }: { admin: MailAdmin; health: ReactNode }) {
  const save = useSetAdminMail()
  const enabled = admin.mail_enabled
  const toggle = (
    <Button
      type="button"
      variant={enabled ? 'outline' : 'default'}
      disabled={save.isPending}
      onClick={async () => {
        const confirmed = await confirmAction(enabled
          ? {
              title: 'Disable Mail?',
              description: 'Mail is hidden for everyone and the workspace mail API refuses. Mail for domains added before still arrives and shows again when you enable it.',
              confirmLabel: 'Disable',
              danger: true,
            }
          : {
              title: 'Enable Mail?',
              description: 'Every workspace gets the Mail app and its settings.',
              confirmLabel: 'Enable',
            })
        if (!confirmed) return
        save.mutate({ mail_enabled: !enabled }, {
          onSuccess: () => toast(enabled ? 'Mail is off' : 'Mail is on'),
          onError: (error) => toast.error(mailProblem(error, 'The setting could not be saved.')),
        })
      }}
    >
      {enabled ? 'Disable' : 'Enable'}
    </Button>
  )
  return (
    <SettingsCard
      title="Mail (Beta)"
      description="Orbit receives and sends mail for the verified domains of each workspace, through its own SMTP listener or through a mail provider. Enabled: every workspace gets the Mail app and its settings. Disabled: Mail is hidden for everyone and the workspace mail API refuses. Mail for domains added before still arrives and shows again when you enable it."
      actions={<div className="flex items-center gap-2">{health}{toggle}</div>}
      flush
    >
      <div className="flex flex-col divide-y">
        <SettingsRow className="flex-nowrap">
          <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
            Workspaces can add their own providers
            <InfoTip text="Owners and admins connect their own PostShiba, Cloudflare, Postmark, Mailgun, Resend or SES account, and their own send routes. Off: no new ones; existing ones keep working." />
          </span>
          <Switch
            aria-label="Workspaces can add their own providers"
            checked={admin.workspace_providers_enabled}
            disabled={save.isPending}
            onCheckedChange={(checked: boolean) => save.mutate({ workspace_providers_enabled: checked }, {
              onSuccess: () => toast(checked ? 'Workspaces can add their own providers' : 'Workspaces cannot add their own providers'),
              onError: (error) => toast.error(mailProblem(error, 'The setting could not be saved.')),
            })}
          />
        </SettingsRow>
        <SettingsRow>
          <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
            Mail hostname
            <InfoTip text="From ORBIT_MAIL_HOSTNAME: the SMTP greeting, the MX target, the name the certificate must cover and, for direct sending, the name of the sending address. Without it, Orbit cannot send directly." />
          </span>
          <code className="font-mono text-[0.8125rem]">{admin.hostname ?? 'Not set'}</code>
        </SettingsRow>
        <SettingsRow>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[0.8125rem]">
            <span className="font-medium text-foreground">Mode</span>
            <span className="text-xs text-muted-foreground">
              {admin.mode === 'relay'
                ? 'Relay: a trusted mail service receives the mail and sends it to Orbit, which uses its authentication results.'
                : 'Direct: the MX record points to Orbit, which checks SPF, DKIM and DMARC itself.'}
            </span>
          </div>
          <Badge variant="outline">{admin.mode === 'relay' ? 'Relay' : 'Direct'}</Badge>
        </SettingsRow>
      </div>
    </SettingsCard>
  )
}

function ptrItem(ptr: PtrStatus, hostname: string | null | undefined): HealthItem {
  const label = 'Reverse DNS (PTR)'
  const addresses = ptr.addresses.map((address) => `${address.ip} → ${address.names.join(', ') || 'no PTR'}`).join('; ')
  switch (ptr.status) {
    case 'ok':
      return { label, state: 'ok', detail: `Every address of ${hostname} points back to it. ${addresses}` }
    case 'mismatch':
      return { label, state: 'warning', detail: addresses, fix: `Ask your hosting provider to set the PTR record of each address to ${hostname}.` }
    case 'no_address':
      return { label, state: 'error', detail: `${hostname} has no A or AAAA record.`, fix: `Add an A (and AAAA) record for ${hostname} with this server's public address.` }
    case 'unknown':
      return { label, state: 'warning', detail: 'The DNS lookup did not answer in time.', fix: 'Reload the page to look again. Check ORBIT_MAIL_DNS_RESOLVER if it keeps failing.' }
    case 'no_hostname':
      return { label, state: 'error', detail: 'No mail hostname.', fix: 'Set ORBIT_MAIL_HOSTNAME.' }
  }
}

function healthItems(admin: MailAdmin, hasProviders: boolean): HealthItem[] {
  if (!admin.enabled && hasProviders) {
    return [{
      label: 'Mail on this server',
      state: 'info',
      detail: 'Mail arrives through providers. The SMTP listener is off, so port 25, the certificate and reverse DNS do not matter here.',
    }]
  }
  if (!admin.enabled) {
    return [{
      label: 'Mail on this server',
      state: 'info',
      detail: 'The SMTP listener is off.',
      fix: 'Add an incoming provider in Providers (no port 25 needed), or set ORBIT_MAIL_ENABLED=true and ORBIT_MAIL_HOSTNAME and restart Orbit. See the operations guide.',
    }]
  }
  const { listener } = admin
  const items: HealthItem[] = [
    listener.running
      ? { label: 'Listener', state: 'ok', detail: `Accepts connections on ${listener.address ?? 'its port'}.` }
      : { label: 'Listener', state: 'error', detail: 'Not running.', fix: 'Read the server log. The address may be in use: check ORBIT_MAIL_SMTP_BIND and ORBIT_MAIL_SMTP_PORT.' },
  ]
  const cert = listener.cert
  if (!cert) {
    items.push(admin.mode === 'direct'
      ? { label: 'Certificate', state: 'error', detail: 'No certificate is loaded.', fix: 'Set ORBIT_MAIL_TLS_CERT and ORBIT_MAIL_TLS_KEY to PEM files that UID 65532 can read.' }
      : { label: 'Certificate', state: 'info', detail: 'None: Orbit offers no STARTTLS. That is fine when the relay is on a private network.', fix: 'Optional in relay mode: set ORBIT_MAIL_TLS_CERT and ORBIT_MAIL_TLS_KEY.' })
  } else if (!cert.covers_hostname) {
    items.push({ label: 'Certificate', state: 'error', detail: `Names ${cert.names.join(', ')}, not ${admin.hostname}.`, fix: `Get a certificate for ${admin.hostname}, for example with certbot. Orbit loads new files within a minute.` })
  } else if (cert.expires_soon) {
    items.push({ label: 'Certificate', state: 'warning', detail: `Expires ${dateTime(cert.expires_at)}.`, fix: 'Renew it. Orbit loads the new files within a minute, without a restart.' })
  } else {
    items.push({ label: 'Certificate', state: 'ok', detail: `Covers ${admin.hostname}, expires ${dateTime(cert.expires_at)}.` })
  }
  if (listener.bridge_peers > 0) {
    items.push({
      label: 'Client address',
      state: 'error',
      detail: `${listener.bridge_peers} ${listener.bridge_peers === 1 ? 'connection' : 'connections'} came from a Docker bridge address, so the real client address is lost.`,
      fix: 'Set "userland-proxy": false in /etc/docker/daemon.json, or run Orbit with network_mode: host. Behind a load balancer, use PROXY protocol v2 and ORBIT_MAIL_PROXY_CIDRS.',
    })
  } else if (listener.last_peer_ip) {
    items.push({ label: 'Client address', state: 'ok', detail: `The last client was ${listener.last_peer_ip}.` })
  } else {
    items.push({ label: 'Client address', state: 'info', detail: 'No client has connected since the start.', fix: 'Send a test message below to check it.' })
  }
  items.push(ptrItem(admin.ptr, admin.hostname))
  return items
}

const STATE_ORDER: HealthState[] = ['error', 'warning', 'info', 'ok']

/** The header button shows the worst state; the dialog lists every check. */
function HealthButton({ admin, hasProviders, refreshing, onRefresh }: { admin: MailAdmin; hasProviders: boolean; refreshing: boolean; onRefresh: () => void }) {
  const items = healthItems(admin, hasProviders)
  const worst = STATE_ORDER.find((state) => items.some((item) => item.state === state)) ?? 'ok'
  const WorstIcon = STATE_ICONS[worst]
  return (
    <Dialog>
      <DialogTrigger render={<Button type="button" variant="outline" data-state={worst} className="group" />}>
        <WorstIcon aria-hidden className={STATE_ICON_COLORS} />
        Health
        <span className="sr-only">: {STATE_WORDS[worst]}</span>
      </DialogTrigger>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Health</DialogTitle>
          <DialogDescription>This server's listener, certificate, client address and reverse DNS. With several servers, each one reports its own.</DialogDescription>
        </DialogHeader>
        <ul className="-mx-4 my-0 flex list-none flex-col divide-y border-t p-0">
          {items.map((item) => {
            const Icon = STATE_ICONS[item.state]
            return (
              <li key={item.label} data-slot="health-item" data-state={item.state} className="group flex items-start gap-3 px-4 py-3">
                <Icon aria-hidden className={`mt-0.5 size-4.5 shrink-0 ${STATE_ICON_COLORS}`} />
                <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[0.8125rem]">
                  <span className="font-medium text-foreground">
                    {item.label}
                    <span className="sr-only">: {STATE_WORDS[item.state]}</span>
                  </span>
                  <span className="text-xs break-words text-muted-foreground">{item.detail}</span>
                  {item.fix && item.state !== 'ok' ? <span className="text-xs text-foreground">{item.fix}</span> : null}
                </div>
              </li>
            )
          })}
        </ul>
        <DialogFooter>
          <Button type="button" variant="outline" disabled={refreshing} onClick={onRefresh}>{refreshing ? 'Checking…' : 'Check again'}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
