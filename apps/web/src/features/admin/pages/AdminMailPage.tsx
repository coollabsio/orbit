import type { ReactNode } from 'react'
import { toast } from 'sonner'
import { CloseCircle, InfoCircle, TickCircle, Warning } from 'reicon-react'
import type { MailAdmin, PtrStatus } from '@/api/generated/types.gen'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useSetAdminMail } from '@/features/mail/api/mutations'
import { useAdminMail } from '@/features/mail/api/queries'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { dateTime, mailProblem } from '@/features/settings/pages/mail/mailSettingsLib'
import { MailTestCard } from '@/features/admin/components/MailTestCard'

type HealthState = 'ok' | 'warning' | 'error' | 'info'

interface HealthItem {
  label: string
  state: HealthState
  detail: ReactNode
  /** What to do about it; shown when the item is not OK. */
  fix?: ReactNode
}

const STATE_ICONS: Record<HealthState, typeof TickCircle> = { ok: TickCircle, warning: Warning, error: CloseCircle, info: InfoCircle }
const STATE_WORDS: Record<HealthState, string> = { ok: 'OK', warning: 'Warning', error: 'Problem', info: 'Note' }

/** The instance's mail: whether workspaces can add domains, the listener's health, and a test message. */
export function AdminMailPage() {
  const admin = useAdminMail()
  if (admin.isPending) return <SettingsCard title="Mail"><p role="status">Loading mail settings…</p></SettingsCard>
  if (admin.isError) {
    return <SettingsCard title="Mail"><p role="alert">{mailProblem(admin.error, 'Mail settings could not be loaded.')} <Button variant="ghost" onClick={() => void admin.refetch()}>Retry</Button></p></SettingsCard>
  }
  return (
    <>
      <DomainsCard admin={admin.data} />
      <HealthCard admin={admin.data} refreshing={admin.isFetching} onRefresh={() => void admin.refetch()} />
      <MailTestCard admin={admin.data} />
    </>
  )
}

function DomainsCard({ admin }: { admin: MailAdmin }) {
  const save = useSetAdminMail()
  return (
    <SettingsCard title="Mail" description="Orbit receives mail for the verified domains of each workspace through its own SMTP listener." flush>
      <div className="flex flex-col divide-y">
        <SettingsRow className="flex-nowrap">
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
            <span className="font-medium text-foreground">Workspaces can add mail domains</span>
            <span className="text-xs text-muted-foreground">Off: owners and admins cannot add a new domain. Domains added before keep working.</span>
          </div>
          <Switch
            aria-label="Workspaces can add mail domains"
            checked={admin.domains_enabled}
            disabled={save.isPending}
            onCheckedChange={(checked: boolean) => save.mutate(checked, {
              onSuccess: () => toast(checked ? 'Workspaces can add mail domains' : 'Workspaces cannot add mail domains'),
              onError: (error) => toast.error(mailProblem(error, 'The setting could not be saved.')),
            })}
          />
        </SettingsRow>
        <SettingsRow>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
            <span className="font-medium text-foreground">Mail hostname</span>
            <span className="text-xs text-muted-foreground">From <code className="font-mono">ORBIT_MAIL_HOSTNAME</code>: the SMTP greeting, the MX target and the name the certificate must cover.</span>
          </div>
          <code className="font-mono text-[13px]">{admin.hostname ?? 'Not set'}</code>
        </SettingsRow>
        <SettingsRow>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
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

function healthItems(admin: MailAdmin): HealthItem[] {
  if (!admin.enabled) {
    return [{
      label: 'Mail on this server',
      state: 'info',
      detail: 'The SMTP listener is off.',
      fix: 'Set ORBIT_MAIL_ENABLED=true and ORBIT_MAIL_HOSTNAME, then restart Orbit. See the operations guide for the mode and port 25.',
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

function HealthCard({ admin, refreshing, onRefresh }: { admin: MailAdmin; refreshing: boolean; onRefresh: () => void }) {
  const items = healthItems(admin)
  return (
    <SettingsCard
      title="Health"
      description="This server's listener, certificate, client address and reverse DNS. With several servers, each one reports its own."
      actions={<Button type="button" variant="outline" disabled={refreshing} onClick={onRefresh}>{refreshing ? 'Checking…' : 'Check again'}</Button>}
      flush
    >
      <ul className="m-0 flex list-none flex-col divide-y p-0">
        {items.map((item) => {
          const Icon = STATE_ICONS[item.state]
          return (
            <li key={item.label} data-slot="health-item" data-state={item.state} className="group flex items-start gap-3 px-4 py-3">
              <Icon
                aria-hidden
                className="mt-0.5 size-4.5 shrink-0 text-muted-foreground group-data-[state=error]:text-destructive group-data-[state=ok]:text-emerald-600 group-data-[state=warning]:text-amber-600 dark:group-data-[state=ok]:text-emerald-400 dark:group-data-[state=warning]:text-amber-400"
              />
              <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
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
    </SettingsCard>
  )
}
