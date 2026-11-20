import type { ReactNode } from 'react'
import { toast } from 'sonner'
import type { MailCertificate, MailCertificateMethod } from '@orbit/api/generated/types.gen'
import { InfoTip } from '@/components/common/InfoTip'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { useCancelMailCertificateOrder, useStartMailCertificate, useVerifyMailCertificate } from '@/features/mail/api/mutations'
import { useMailCertificate } from '@/features/mail/api/queries'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { CopyButton } from '@/features/settings/pages/mail/MailDomainRecords'
import { dateTime, mailProblem } from '@/features/settings/pages/mail/mailSettingsLib'

const METHOD_WORDS: Record<MailCertificateMethod, string> = { http: 'HTTP check', dns: 'DNS record' }

function Row({ label, tip, children }: { label: string; tip?: string; children: ReactNode }) {
  return (
    <SettingsRow>
      <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem] font-medium text-foreground">
        {label}
        {tip ? <InfoTip text={tip} /> : null}
      </span>
      <span className="min-w-0 text-right text-[0.8125rem] break-words text-muted-foreground">{children}</span>
    </SettingsRow>
  )
}

/**
 * **Certificate**: the SMTP listener's certificate for `STARTTLS`. Orbit gets one from Let's Encrypt for the mail
 * hostname: an HTTP check through the reverse proxy first, a TXT record as the fallback. Files from
 * `ORBIT_MAIL_TLS_CERT` / `ORBIT_MAIL_TLS_KEY` win and make this card read only.
 */
export function MailCertificateCard() {
  const certificate = useMailCertificate()
  const description = 'The certificate the SMTP listener offers for STARTTLS. Orbit gets it from Let’s Encrypt for the mail hostname and renews it automatically with the HTTP check. Every server of a cluster loads it within a minute.'
  if (certificate.isPending) return <SettingsCard title="Certificate" description={description}><p role="status" className="m-0 text-[0.8125rem]">Loading the certificate…</p></SettingsCard>
  if (certificate.isError) {
    return (
      <SettingsCard title="Certificate" description={description}>
        <p role="alert" className="m-0 text-[0.8125rem]">{mailProblem(certificate.error, 'The certificate could not be loaded.')} <Button variant="ghost" onClick={() => void certificate.refetch()}>Retry</Button></p>
      </SettingsCard>
    )
  }
  return <CertificateBody certificate={certificate.data} description={description} />
}

function CertificateBody({ certificate, description }: { certificate: MailCertificate; description: string }) {
  const start = useStartMailCertificate()
  const verify = useVerifyMailCertificate()
  const cancel = useCancelMailCertificateOrder()
  const busy = start.isPending || verify.isPending || cancel.isPending
  const { active, pending, hostname } = certificate
  const fromEnvironment = certificate.source === 'environment'
  const request = (method: MailCertificateMethod) => start.mutate(method, {
    onSuccess: (next) => {
      if (next.last_error) toast.error('No certificate. See the error in the card.')
      else if (next.pending?.method === 'dns') toast('Add the TXT record, then verify it')
      else toast('Certificate loaded')
    },
    onError: (error) => toast.error(mailProblem(error, 'The certificate could not be requested.')),
  })
  const actions = fromEnvironment || !hostname || pending?.method === 'dns' ? null : (
    <>
      <Button type="button" variant="outline" disabled={busy} onClick={() => request('dns')}>Use a DNS record</Button>
      <Button type="button" disabled={busy} onClick={() => request('http')}>
        {start.isPending && start.variables === 'http' ? <><Spinner /> Getting a certificate…</> : active ? 'Renew now' : 'Get certificate'}
      </Button>
    </>
  )

  return (
    <SettingsCard title="Certificate" description={description} actions={actions} flush>
      <div className="flex flex-col divide-y">
        {fromEnvironment ? (
          <SettingsRow>
            <p className="m-0 text-[0.8125rem] text-muted-foreground">
              From the files in ORBIT_MAIL_TLS_CERT and ORBIT_MAIL_TLS_KEY. Orbit loads new files within a minute. Remove both settings to let Orbit get the certificate.
            </p>
          </SettingsRow>
        ) : !hostname ? (
          <SettingsRow>
            <p className="m-0 text-[0.8125rem] text-muted-foreground">Set ORBIT_MAIL_HOSTNAME and restart Orbit to get a certificate.</p>
          </SettingsRow>
        ) : null}
        {active ? (
          <>
            <Row label="Names">
              {active.names.join(', ')}
              {active.covers_hostname ? null : <Badge variant="destructive" className="ml-2">Not {hostname}</Badge>}
            </Row>
            <Row label="Expires">{dateTime(active.not_after)}</Row>
            {active.method ? (
              <Row label="Checked with" tip="How Let’s Encrypt checked that this server owns the name. Automatic renewal always uses the HTTP check.">
                {METHOD_WORDS[active.method]}{active.issued_at ? `, ${dateTime(active.issued_at)}` : ''}
              </Row>
            ) : null}
            {certificate.renew_after ? <Row label="Renews after">{dateTime(certificate.renew_after)}</Row> : null}
          </>
        ) : !fromEnvironment && hostname ? (
          <SettingsRow>
            <p className="m-0 text-[0.8125rem] text-muted-foreground">
              No certificate yet, so the listener offers no STARTTLS.
              <InfoTip text={`The HTTP check needs port 80 of ${hostname} to reach the reverse proxy, which sends /.well-known/acme-challenge/ to Orbit. Without that, use a DNS record: you add a TXT record once, and again for each renewal.`} />
            </p>
          </SettingsRow>
        ) : null}
        {pending?.method === 'dns' && pending.dns_name && pending.dns_value ? (
          <DnsStep
            name={pending.dns_name}
            value={pending.dns_value}
            busy={busy}
            verifying={verify.isPending}
            onVerify={() => verify.mutate(undefined, {
              onSuccess: (next) => (next.last_error ? toast.error('No certificate. See the error in the card.') : toast('Certificate loaded')),
              onError: (error) => toast.error(mailProblem(error, 'The record could not be verified.')),
            })}
            onCancel={() => cancel.mutate(undefined, { onError: (error) => toast.error(mailProblem(error, 'The request could not be cancelled.')) })}
          />
        ) : null}
        {pending?.method === 'http' && !start.isPending ? (
          <SettingsRow><p role="status" className="m-0 text-[0.8125rem] text-muted-foreground">Let’s Encrypt is checking {hostname} over HTTP.</p></SettingsRow>
        ) : null}
        {certificate.last_error ? (
          <SettingsRow>
            <div className="flex min-w-0 flex-1 flex-col gap-1 text-[0.8125rem]">
              <p role="alert" className="m-0 text-destructive">
                The last request failed{certificate.last_attempt_at ? ` (${dateTime(certificate.last_attempt_at)})` : ''}: {certificate.last_error}
              </p>
              <p className="m-0 text-muted-foreground">Check that port 80 of {hostname} reaches the reverse proxy, or use a DNS record.</p>
            </div>
          </SettingsRow>
        ) : null}
      </div>
    </SettingsCard>
  )
}

function DnsStep({ name, value, busy, verifying, onVerify, onCancel }: {
  name: string
  value: string
  busy: boolean
  verifying: boolean
  onVerify: () => void
  onCancel: () => void
}) {
  return (
    <div className="flex flex-col gap-3 px-4 py-3 text-[0.8125rem]" data-slot="certificate-dns-step">
      <p className="m-0 text-foreground">
        Add this TXT record at your DNS provider, then click Verify.
        <InfoTip text="Orbit looks the record up before it asks Let’s Encrypt, so a record that is not visible yet does not use up the check. DNS can take a few minutes. You can delete the record once the certificate is loaded." />
      </p>
      <dl className="m-0 grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-x-3 gap-y-1">
        <dt className="text-muted-foreground">Name</dt>
        <dd className="m-0 font-mono text-xs break-all">{name}</dd>
        <dd className="m-0"><CopyButton text={name} label="Name" /></dd>
        <dt className="text-muted-foreground">Type</dt>
        <dd className="m-0 font-mono text-xs">TXT</dd>
        <dd className="m-0" />
        <dt className="text-muted-foreground">Value</dt>
        <dd className="m-0 font-mono text-xs break-all">{value}</dd>
        <dd className="m-0"><CopyButton text={value} label="Value" /></dd>
      </dl>
      <div className="flex flex-wrap gap-2">
        <Button type="button" disabled={busy} onClick={onVerify}>{verifying ? <><Spinner /> Verifying…</> : 'Verify'}</Button>
        <Button type="button" variant="outline" disabled={busy} onClick={onCancel}>Cancel</Button>
      </div>
    </div>
  )
}
