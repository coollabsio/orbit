import { type ReactNode, useState } from 'react'
import { View } from 'react-native'

import type { MailAuthResult, MailCertificateMethod } from '@orbit/api/generated/types.gen'
import { CopyButton } from '../../../../components/settings/mail/DomainRecords'
import { ProviderConnections } from '../../../../components/settings/mail/ProviderConnections'
import { ConfirmDialog, Dialog, type ConfirmRequest } from '../../../../components/settings/Dialog'
import { AdminShell, Badge, type BadgeTone, Note, OptionHint, OptionText, Select, SettingsCard, SettingsRow, Toggle } from '../../../../components/settings/Shell'
import { healthItems, type HealthState, type MailAdmin, type MailCertificate, type MailTestAddress, type MailTestResult, STATE_WORDS, worstState } from '../../../../data/admin-mail-api'
import { timeOfDay } from '../../../../data/format'
import { dateTime, mailProblem } from '../../../../data/mail-settings'
import { useAdminMail, useCertificateWrites, useCreateTestAddress, useMailCertificate, useSetAdminMail, useTestAddress } from '../../../../hooks/admin-mail'
import { useMailProviders, useMailSendRoutes } from '../../../../hooks/mail-domains'
import { presenceColors, useTheme } from '../../../../theme'
import { Button, Icon, type IconName, LabelTip, Spinner, Text } from '../../../../ui'

/** A result line under a card's rows: the web shows these as toasts. */
type Notice = { text: string; error?: boolean } | null

const STATE_ICONS: Record<HealthState, IconName> = { ok: 'checkCircle', warning: 'warning', error: 'closeCircle', info: 'infoCircle' }

function useStateColor() {
  const { colors } = useTheme()
  return (state: HealthState) => ({ ok: presenceColors.online, warning: presenceColors.idle, error: colors.destructive, info: colors.mutedForeground })[state]
}

/**
 * The web's Admin → Mail: the switches with the health button, the certificate, receiving and sending (providers and
 * routes) and a test message. The send limits are on the Limits page.
 */
export default function AdminMailScreen() {
  const admin = useAdminMail()
  const providers = useMailProviders('instance')
  const routes = useMailSendRoutes('instance')
  const hasProviders = (providers.data?.providers.length ?? 0) > 0
  return (
    <AdminShell onRefresh={() => Promise.all([admin.refetch(), providers.refetch(), routes.refetch()])}>
      {admin.isPending ? <SettingsCard title="Mail"><Note loading>Loading mail settings…</Note></SettingsCard> : null}
      {admin.isError ? (
        <SettingsCard title="Mail">
          <View className="flex-row flex-wrap items-center gap-2">
            <Note error>{mailProblem(admin.error, 'Mail settings could not be loaded.')}</Note>
            <Button label="Retry" variant="ghost" onPress={() => void admin.refetch()} />
          </View>
        </SettingsCard>
      ) : null}
      {admin.data ? (
        <>
          <DomainsCard admin={admin.data} hasProviders={hasProviders} refreshing={admin.isFetching} onRefresh={() => void admin.refetch()} />
          {admin.data.enabled ? <CertificateCard /> : null}
          <ProviderConnections scope="instance" hostname={admin.data.hostname} />
          <TestCard admin={admin.data} hasProviders={hasProviders} />
        </>
      ) : null}
    </AdminShell>
  )
}

function DomainsCard({ admin, hasProviders, refreshing, onRefresh }: { admin: MailAdmin; hasProviders: boolean; refreshing: boolean; onRefresh: () => void }) {
  const save = useSetAdminMail()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const [notice, setNotice] = useState<Notice>(null)
  const enabled = admin.mail_enabled
  const saveSwitch = (body: Parameters<typeof save.mutate>[0], done: string) => {
    setNotice(null)
    save.mutate(body, {
      onSuccess: () => setNotice({ text: done }),
      onError: (error) => setNotice({ text: mailProblem(error, 'The setting could not be saved.'), error: true }),
    })
  }
  const toggle = () =>
    setConfirm(enabled
      ? {
          title: 'Disable Mail?',
          description: 'Mail is hidden for everyone and the workspace mail API refuses. Mail for domains added before still arrives and shows again when you enable it.',
          confirmLabel: 'Disable',
          danger: true,
          onConfirm: () => saveSwitch({ mail_enabled: false }, 'Mail is off'),
        }
      : {
          title: 'Enable Mail?',
          description: 'Every workspace gets the Mail app and its settings.',
          confirmLabel: 'Enable',
          onConfirm: () => saveSwitch({ mail_enabled: true }, 'Mail is on'),
        })

  return (
    <SettingsCard
      title="Mail (Beta)"
      description="Orbit receives and sends mail for the verified domains of each workspace, through its own SMTP listener or through a mail provider. Enabled: every workspace gets the Mail app and its settings. Disabled: Mail is hidden for everyone and the workspace mail API refuses. Mail for domains added before still arrives and shows again when you enable it."
      actions={
        <>
          <HealthButton admin={admin} hasProviders={hasProviders} refreshing={refreshing} onRefresh={onRefresh} />
          <Button label={enabled ? 'Disable' : 'Enable'} variant={enabled ? 'outline' : 'primary'} disabled={save.isPending} onPress={toggle} />
        </>
      }
      flush
    >
      <SettingsRow first>
        <OptionText
          title="Workspaces can add their own providers"
          help="Owners and admins connect their own PostShiba, Cloudflare, Postmark, Mailgun, Resend or SES account, and their own send routes. Off: no new ones; existing ones keep working."
        />
        <Toggle
          label="Workspaces can add their own providers"
          value={admin.workspace_providers_enabled}
          disabled={save.isPending}
          onChange={(checked) => saveSwitch({ workspace_providers_enabled: checked }, checked ? 'Workspaces can add their own providers' : 'Workspaces cannot add their own providers')}
        />
      </SettingsRow>
      <SettingsRow>
        <View className="flex-1 min-w-0">
          <LabelTip
            label="Mail hostname"
            text="From ORBIT_MAIL_HOSTNAME: the SMTP greeting, the MX target, the name the certificate must cover and, for direct sending, the name of the sending address. Without it, Orbit cannot send directly."
            className="font-noto-medium text-[13px]"
          />
        </View>
        <Text selectable className="shrink font-mono text-[13px]">{admin.hostname ?? 'Not set'}</Text>
      </SettingsRow>
      <SettingsRow>
        <OptionText title="Mode">
          <OptionHint>
            {admin.mode === 'relay'
              ? 'Relay: a trusted mail service receives the mail and sends it to Orbit, which uses its authentication results.'
              : 'Direct: the MX record points to Orbit, which checks SPF, DKIM and DMARC itself.'}
          </OptionHint>
        </OptionText>
        <Badge label={admin.mode === 'relay' ? 'Relay' : 'Direct'} tone="outline" />
      </SettingsRow>
      {notice ? <SettingsRow><Note error={notice.error}>{notice.text}</Note></SettingsRow> : null}
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsCard>
  )
}

/** The header button shows the worst state; the dialog lists every check. */
function HealthButton({ admin, hasProviders, refreshing, onRefresh }: { admin: MailAdmin; hasProviders: boolean; refreshing: boolean; onRefresh: () => void }) {
  const stateColor = useStateColor()
  const [open, setOpen] = useState(false)
  const items = healthItems(admin, hasProviders)
  const worst = worstState(items)
  return (
    <>
      <Button label="Health" icon={STATE_ICONS[worst]} variant={worst === 'error' ? 'destructive' : 'outline'} onPress={() => setOpen(true)} />
      <Dialog
        visible={open}
        title="Health"
        description="This server's listener, certificate, client address and reverse DNS. With several servers, each one reports its own."
        onClose={() => setOpen(false)}
        footer={
          <>
            <Button label={refreshing ? 'Checking…' : 'Check again'} variant="outline" disabled={refreshing} onPress={onRefresh} />
            <Button label="Close" onPress={() => setOpen(false)} />
          </>
        }
      >
        <View className="border-t border-border">
          {items.map((item) => (
            <View key={item.label} accessibilityLabel={`${item.label}: ${STATE_WORDS[item.state]}`} className="flex-row items-start gap-3 border-b border-border py-3">
              <View className="mt-0.5">
                <Icon name={STATE_ICONS[item.state]} size={18} color={stateColor(item.state)} />
              </View>
              <View className="flex-1 min-w-0 gap-0.5">
                <Text className="font-noto-medium text-[13px]">{item.label}</Text>
                <Text className="text-[12px] leading-[17px] text-muted-foreground">{item.detail}</Text>
                {item.fix && item.state !== 'ok' ? <Text className="text-[12px] leading-[17px] text-foreground">{item.fix}</Text> : null}
              </View>
            </View>
          ))}
        </View>
      </Dialog>
    </>
  )
}

const CERTIFICATE_DESCRIPTION =
  'The certificate the SMTP listener offers for STARTTLS. Orbit gets it from Let’s Encrypt for the mail hostname and renews it automatically with the HTTP check. Every server of a cluster loads it within a minute.'

const METHOD_WORDS: Record<MailCertificateMethod, string> = { http: 'HTTP check', dns: 'DNS record' }

/**
 * **Certificate**: Orbit gets one from Let's Encrypt for the mail hostname, with an HTTP check first and a TXT record
 * as the fallback. Files from `ORBIT_MAIL_TLS_CERT` / `ORBIT_MAIL_TLS_KEY` win and make this card read only.
 */
function CertificateCard() {
  const certificate = useMailCertificate()
  if (certificate.isPending) return <SettingsCard title="Certificate" description={CERTIFICATE_DESCRIPTION}><Note loading>Loading the certificate…</Note></SettingsCard>
  if (certificate.isError) {
    return (
      <SettingsCard title="Certificate" description={CERTIFICATE_DESCRIPTION}>
        <View className="flex-row flex-wrap items-center gap-2">
          <Note error>{mailProblem(certificate.error, 'The certificate could not be loaded.')}</Note>
          <Button label="Retry" variant="ghost" onPress={() => void certificate.refetch()} />
        </View>
      </SettingsCard>
    )
  }
  return <CertificateBody certificate={certificate.data} />
}

function CertRow({ label, tip, children, first = false }: { label: string; tip?: string; children: ReactNode; first?: boolean }) {
  return (
    <SettingsRow first={first}>
      <View className="flex-1 min-w-0">
        {tip ? <LabelTip label={label} text={tip} className="font-noto-medium text-[13px]" /> : <Text className="font-noto-medium text-[13px]">{label}</Text>}
      </View>
      <View className="shrink items-end">{children}</View>
    </SettingsRow>
  )
}

function CertificateBody({ certificate }: { certificate: MailCertificate }) {
  const { start, verify, cancel } = useCertificateWrites()
  const [notice, setNotice] = useState<Notice>(null)
  const busy = start.isPending || verify.isPending || cancel.isPending
  const { active, pending, hostname } = certificate
  const fromEnvironment = certificate.source === 'environment'
  const failed = (fallback: string) => (error: unknown) => setNotice({ text: mailProblem(error, fallback), error: true })
  const loaded = (next: MailCertificate) => setNotice(next.last_error ? { text: 'No certificate. See the error in the card.', error: true } : { text: 'Certificate loaded' })
  const request = (method: MailCertificateMethod) => {
    setNotice(null)
    start.mutate(method, {
      onSuccess: (next) => (next.last_error || next.pending?.method !== 'dns' ? loaded(next) : setNotice({ text: 'Add the TXT record, then verify it' })),
      onError: failed('The certificate could not be requested.'),
    })
  }
  const actions = fromEnvironment || !hostname || pending?.method === 'dns' ? undefined : (
    <>
      <Button label="Use a DNS record" variant="outline" disabled={busy} onPress={() => request('dns')} />
      <Button
        label={start.isPending && start.variables === 'http' ? 'Getting a certificate…' : active ? 'Renew now' : 'Get certificate'}
        loading={start.isPending && start.variables === 'http'}
        disabled={busy}
        onPress={() => request('http')}
      />
    </>
  )
  const muted = 'text-[13px] leading-[19px] text-muted-foreground'

  return (
    <SettingsCard title="Certificate" description={CERTIFICATE_DESCRIPTION} actions={actions} flush>
      <>
        {fromEnvironment ? (
          <SettingsRow first>
            <Text className={`flex-1 ${muted}`}>From the files in ORBIT_MAIL_TLS_CERT and ORBIT_MAIL_TLS_KEY. Orbit loads new files within a minute. Remove both settings to let Orbit get the certificate.</Text>
          </SettingsRow>
        ) : !hostname ? (
          <SettingsRow first>
            <Text className={`flex-1 ${muted}`}>Set ORBIT_MAIL_HOSTNAME and restart Orbit to get a certificate.</Text>
          </SettingsRow>
        ) : null}
        {active ? (
          <>
            <CertRow label="Names" first={!fromEnvironment && !!hostname}>
              <Text className={`text-right ${muted}`}>{active.names.join(', ')}</Text>
              {active.covers_hostname ? null : <Badge label={`Not ${hostname}`} tone="destructive" />}
            </CertRow>
            <CertRow label="Expires"><Text className={`text-right ${muted}`}>{dateTime(active.not_after)}</Text></CertRow>
            {active.method ? (
              <CertRow label="Checked with" tip="How Let’s Encrypt checked that this server owns the name. Automatic renewal always uses the HTTP check.">
                <Text className={`text-right ${muted}`}>{METHOD_WORDS[active.method]}{active.issued_at ? `, ${dateTime(active.issued_at)}` : ''}</Text>
              </CertRow>
            ) : null}
            {certificate.renew_after ? <CertRow label="Renews after"><Text className={`text-right ${muted}`}>{dateTime(certificate.renew_after)}</Text></CertRow> : null}
          </>
        ) : !fromEnvironment && hostname ? (
          <SettingsRow first>
            <View className="flex-1 min-w-0">
              <LabelTip
                label="No certificate yet, so the listener offers no STARTTLS."
                text={`The HTTP check needs port 80 of ${hostname} to reach the reverse proxy, which sends /.well-known/acme-challenge/ to Orbit. Without that, use a DNS record: you add a TXT record once, and again for each renewal.`}
                className={muted}
              />
            </View>
          </SettingsRow>
        ) : null}
        {pending?.method === 'dns' && pending.dns_name && pending.dns_value ? (
          <DnsStep
            name={pending.dns_name}
            value={pending.dns_value}
            busy={busy}
            verifying={verify.isPending}
            onVerify={() => {
              setNotice(null)
              verify.mutate(undefined, { onSuccess: loaded, onError: failed('The record could not be verified.') })
            }}
            onCancel={() => {
              setNotice(null)
              cancel.mutate(undefined, { onError: failed('The request could not be cancelled.') })
            }}
          />
        ) : null}
        {pending?.method === 'http' && !start.isPending ? (
          <SettingsRow><Text accessibilityRole="text" className={`flex-1 ${muted}`}>Let’s Encrypt is checking {hostname} over HTTP.</Text></SettingsRow>
        ) : null}
        {certificate.last_error ? (
          <SettingsRow>
            <View className="flex-1 min-w-0 gap-1">
              <Text accessibilityRole="alert" className="text-[13px] leading-[19px] text-destructive">
                The last request failed{certificate.last_attempt_at ? ` (${dateTime(certificate.last_attempt_at)})` : ''}: {certificate.last_error}
              </Text>
              <Text className={muted}>Check that port 80 of {hostname} reaches the reverse proxy, or use a DNS record.</Text>
            </View>
          </SettingsRow>
        ) : null}
        {notice ? <SettingsRow><Note error={notice.error}>{notice.text}</Note></SettingsRow> : null}
      </>
    </SettingsCard>
  )
}

function DnsStep({ name, value, busy, verifying, onVerify, onCancel }: { name: string; value: string; busy: boolean; verifying: boolean; onVerify: () => void; onCancel: () => void }) {
  return (
    <View className="gap-3 border-t border-border px-4 py-3">
      <LabelTip
        label="Add this TXT record at your DNS provider, then click Verify."
        text="Orbit looks the record up before it asks Let’s Encrypt, so a record that is not visible yet does not use up the check. DNS can take a few minutes. You can delete the record once the certificate is loaded."
        className="text-[13px] leading-[19px] text-foreground"
      />
      <View className="gap-1">
        <RecordLine label="Name" value={name} copy />
        <RecordLine label="Type" value="TXT" />
        <RecordLine label="Value" value={value} copy />
      </View>
      <View className="flex-row flex-wrap gap-2">
        <Button label={verifying ? 'Verifying…' : 'Verify'} loading={verifying} disabled={busy} onPress={onVerify} />
        <Button label="Cancel" variant="outline" disabled={busy} onPress={onCancel} />
      </View>
    </View>
  )
}

function RecordLine({ label, value, copy = false }: { label: string; value: string; copy?: boolean }) {
  return (
    <View className="min-h-7 flex-row items-center gap-3">
      <Text className="w-12 text-[13px] text-muted-foreground">{label}</Text>
      <Text selectable className="flex-1 font-mono text-[12px]">{value}</Text>
      {copy ? <CopyButton text={value} label={label} /> : <View className="size-7" />}
    </View>
  )
}

const RESULT_TONES: Partial<Record<MailAuthResult, BadgeTone>> = { pass: 'secondary', fail: 'destructive', permerror: 'destructive' }

/**
 * **Test message**: a one-time address `check-…@<verified domain>`, valid for an hour. Mail sent to it from outside
 * proves MX, port 25, TLS and the checks; the card asks for the result every 3 s until it arrives or expires.
 */
function TestCard({ admin, hasProviders }: { admin: MailAdmin; hasProviders: boolean }) {
  const create = useCreateTestAddress()
  const [domainId, setDomainId] = useState(admin.test_domains[0]?.id ?? '')
  const [test, setTest] = useState<MailTestAddress | null>(null)
  const result = useTestAddress(test?.id)
  const options = admin.test_domains.map((domain) => ({ value: domain.id, label: domain.name }))
  const chosen = options.some((option) => option.value === domainId) ? domainId : (options[0]?.value ?? '')
  // mail from a provider reaches the test address too, so the listener is needed only without one
  const unavailable = !admin.enabled && !hasProviders
    ? 'Add an incoming provider above, or turn on this server’s SMTP listener (ORBIT_MAIL_ENABLED=true and ORBIT_MAIL_HOSTNAME, then restart Orbit).'
    : options.length === 0
      ? 'No workspace has a verified domain yet. Add one in Settings → Mail → Domains of a workspace.'
      : null

  return (
    <SettingsCard
      title="Test message"
      description="Send a mail from an outside account (for example Gmail) to a one-time address. It does not go to a mailbox; the card shows what Orbit saw."
      flush
    >
      <View className="gap-2 px-4 py-3">
        {unavailable ? <Note>{unavailable}</Note> : (
          <>
            <Select label="Domain of the test address" value={chosen} options={options} onChange={setDomainId} />
            <View className="flex-row">
              <Button
                label={create.isPending ? 'Making an address…' : test ? 'New test address' : 'Make a test address'}
                disabled={!chosen || create.isPending}
                onPress={() => create.mutate(chosen, { onSuccess: setTest })}
              />
            </View>
            {create.isError ? <Note error>{mailProblem(create.error, 'The test address could not be made.')}</Note> : null}
          </>
        )}
      </View>
      {test ? <TestResult test={test} result={result.data} failed={result.isError} /> : null}
    </SettingsCard>
  )
}

function TestResult({ test, result, failed }: { test: MailTestAddress; result: MailTestResult | undefined; failed: boolean }) {
  const status = result?.status ?? 'waiting'
  const okText = useTheme().scheme === 'dark' ? 'text-emerald-400' : 'text-emerald-700'
  return (
    <View className="gap-3 border-t border-border px-4 py-3">
      <View className="flex-row flex-wrap items-center gap-2">
        <Text selectable className="font-mono text-[13px]">{test.address}</Text>
        <CopyButton text={test.address} label="Address" />
        <Text className="text-[12px] text-muted-foreground">Valid until {timeOfDay(test.expires_at)}</Text>
      </View>
      {status === 'waiting' ? (
        <View className="flex-row items-center gap-2">
          {failed ? null : <Spinner />}
          <Note>{failed ? 'The result could not be read. Orbit tries again.' : 'Waiting for the message…'}</Note>
        </View>
      ) : null}
      {status === 'expired' ? <Note>The address expired before a message arrived. Make a new one.</Note> : null}
      {status === 'received' && result ? (
        <View className="gap-2">
          <Text className={`font-noto-medium text-[13px] ${okText}`}>The message arrived{result.received_at ? ` at ${dateTime(result.received_at)}` : ''}.</Text>
          <View className="gap-1">
            <ResultLine label="Client address" value={result.peer_ip ?? 'unknown'} mono />
            <ResultLine label="HELO" value={result.helo ?? '—'} mono />
            <ResultLine label="Sender" value={result.mail_from ?? '<>'} mono />
            <ResultLine label="TLS" value={result.tls ? `${result.tls.version} (${result.tls.cipher})` : 'None: the message came without TLS'} />
          </View>
          {result.auth ? (
            <View className="flex-row flex-wrap items-center gap-1.5">
              <Badge label={`SPF: ${result.auth.spf}`} tone={RESULT_TONES[result.auth.spf] ?? 'outline'} />
              <Badge label={`DKIM: ${result.auth.dkim}`} tone={RESULT_TONES[result.auth.dkim] ?? 'outline'} />
              <Badge label={`DMARC: ${result.auth.dmarc}`} tone={RESULT_TONES[result.auth.dmarc] ?? 'outline'} />
              {result.auth.aligned_domain ? <Text className="text-[12px] text-muted-foreground">Aligned with {result.auth.aligned_domain}</Text> : null}
            </View>
          ) : <Text className="text-[12px] text-muted-foreground">No authentication results were recorded.</Text>}
        </View>
      ) : null}
    </View>
  )
}

function ResultLine({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <View className="flex-row gap-4">
      <Text className="w-28 text-[12px] text-muted-foreground">{label}</Text>
      <Text selectable className={`flex-1 text-[12px] ${mono ? 'font-mono' : ''}`}>{value}</Text>
    </View>
  )
}
