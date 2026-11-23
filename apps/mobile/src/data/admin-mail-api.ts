/**
 * The web's Admin → Mail and Admin → Limits → Email calls (root user only) and their pure rules: the health checks of
 * this server's mail and the send limit forms. No React.
 */
import {
  cancelMailCertificateOrder,
  createMailTestAddress,
  getMailAdmin,
  getMailCertificate,
  getMailSendTiers,
  getMailTestAddress,
  listMailSendPolicies,
  resumeMailSending,
  setMailAdmin,
  setMailSendPolicy,
  setMailSendTiers,
  startMailCertificate,
  verifyMailCertificate,
} from '@orbit/api/generated/sdk.gen'
import type {
  Limits,
  MailAdmin,
  MailCertificate,
  MailCertificateMethod,
  MailSendPolicy,
  MailTestAddress,
  MailTestResult,
  PtrStatus,
  SetMailAdminBody,
  SetMailSendPolicyBody,
  TierLimits,
} from '@orbit/api/generated/types.gen'

import { dateTime } from './mail-settings'
import type { OrbitClient } from './orbit'

export type { Limits, MailAdmin, MailCertificate, MailCertificateMethod, MailSendPolicy, MailTestAddress, MailTestResult, TierLimits }

/** The web's `queryKeys.adminMail*`. */
export const adminMailKeys = {
  admin: ['admin', 'mail'] as const,
  certificate: ['admin', 'mail', 'certificate'] as const,
  sendPolicies: ['admin', 'mail', 'send-policies'] as const,
  sendTiers: ['admin', 'mail', 'send-tiers'] as const,
  test: (testId: string) => ['admin', 'mail', 'test', testId] as const,
}

/** The certificate and a test address are asked for again every 3 s while they wait. */
export const POLL_MS = 3000

export async function mailAdmin(client: OrbitClient): Promise<MailAdmin> {
  return (await getMailAdmin({ client, throwOnError: true })).data
}

/** An absent field stays. */
export async function saveMailAdmin(client: OrbitClient, body: SetMailAdminBody): Promise<MailAdmin> {
  return (await setMailAdmin({ client, body, throwOnError: true })).data
}

export async function mailCertificate(client: OrbitClient): Promise<MailCertificate> {
  return (await getMailCertificate({ client, throwOnError: true })).data
}

/** `http` validates and issues in the request (up to about a minute); `dns` answers with the TXT record to add. */
export async function startCertificate(client: OrbitClient, method: MailCertificateMethod): Promise<MailCertificate> {
  return (await startMailCertificate({ client, body: { method }, throwOnError: true })).data
}

export async function verifyCertificate(client: OrbitClient): Promise<MailCertificate> {
  return (await verifyMailCertificate({ client, throwOnError: true })).data
}

export async function cancelCertificate(client: OrbitClient): Promise<MailCertificate> {
  return (await cancelMailCertificateOrder({ client, throwOnError: true })).data
}

/** A one-time address `check-…@<domain>`, valid for an hour. */
export async function createTestAddress(client: OrbitClient, domainId: string): Promise<MailTestAddress> {
  return (await createMailTestAddress({ client, body: { domain_id: domainId }, throwOnError: true })).data
}

export async function testAddress(client: OrbitClient, testId: string): Promise<MailTestResult> {
  return (await getMailTestAddress({ client, path: { test_id: testId }, throwOnError: true })).data
}

export async function sendTiers(client: OrbitClient): Promise<TierLimits> {
  return (await getMailSendTiers({ client, throwOnError: true })).data
}

export async function saveSendTiers(client: OrbitClient, body: TierLimits): Promise<TierLimits> {
  return (await setMailSendTiers({ client, body, throwOnError: true })).data
}

/** Every workspace's tier, use, rates and pause, paused ones first. */
export async function sendPolicies(client: OrbitClient): Promise<MailSendPolicy[]> {
  return (await listMailSendPolicies({ client, throwOnError: true })).data.items
}

export async function saveSendPolicy(client: OrbitClient, workspaceId: string, body: SetMailSendPolicyBody): Promise<MailSendPolicy> {
  return (await setMailSendPolicy({ client, path: { workspace_id: workspaceId }, body, throwOnError: true })).data
}

export async function resumeSending(client: OrbitClient, workspaceId: string): Promise<MailSendPolicy> {
  return (await resumeMailSending({ client, path: { workspace_id: workspaceId }, throwOnError: true })).data
}

// Health

export type HealthState = 'ok' | 'warning' | 'error' | 'info'

export interface HealthItem {
  label: string
  state: HealthState
  detail: string
  /** What to do about it; shown when the item is not OK. */
  fix?: string
}

export const STATE_WORDS: Record<HealthState, string> = { ok: 'OK', warning: 'Warning', error: 'Problem', info: 'Note' }

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

/** The web's Health dialog: this server's listener, certificate, client address and reverse DNS. */
export function healthItems(admin: MailAdmin, hasProviders: boolean): HealthItem[] {
  // only directly sent mail leaves from this server's address, so only then do receivers check its PTR
  const ptr = admin.direct_sending ? ptrItem(admin.ptr, admin.hostname) : null
  if (!admin.enabled && hasProviders) {
    return [{
      label: 'Mail on this server',
      state: 'info',
      detail: 'Mail arrives through providers. The SMTP listener is off, so port 25 and the certificate do not matter here.',
    }, ...(ptr ? [ptr] : [])]
  }
  if (!admin.enabled) {
    return [{
      label: 'Mail on this server',
      state: 'info',
      detail: 'The SMTP listener is off.',
      fix: 'Add an incoming provider in Providers (no port 25 needed), or set ORBIT_MAIL_ENABLED=true and ORBIT_MAIL_HOSTNAME and restart Orbit. See the operations guide.',
    }, ...(ptr ? [ptr] : [])]
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
      ? { label: 'Certificate', state: 'error', detail: 'No certificate is loaded.', fix: 'Click Get certificate in the Certificate card, or set ORBIT_MAIL_TLS_CERT and ORBIT_MAIL_TLS_KEY to PEM files that UID 65532 can read.' }
      : { label: 'Certificate', state: 'info', detail: 'None: Orbit offers no STARTTLS. That is fine when the relay is on a private network.', fix: 'Optional in relay mode: get one in the Certificate card, or set ORBIT_MAIL_TLS_CERT and ORBIT_MAIL_TLS_KEY.' })
  } else if (!cert.covers_hostname) {
    items.push({ label: 'Certificate', state: 'error', detail: `Names ${cert.names.join(', ')}, not ${admin.hostname}.`, fix: `Get a certificate for ${admin.hostname} in the Certificate card, or with certbot for the files. Orbit loads a new certificate within a minute.` })
  } else if (cert.expires_soon) {
    items.push({ label: 'Certificate', state: 'warning', detail: `Expires ${dateTime(cert.expires_at)}.`, fix: 'Renew it in the Certificate card, or renew the files. Orbit loads the new certificate within a minute, without a restart.' })
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
  items.push(ptr ?? { label: 'Reverse DNS (PTR)', state: 'info', detail: 'Not needed: no outgoing provider sends directly from this server.' })
  return items
}

const STATE_ORDER: HealthState[] = ['error', 'warning', 'info', 'ok']

/** The Health button shows the worst state. */
export const worstState = (items: HealthItem[]): HealthState => STATE_ORDER.find((state) => items.some((item) => item.state === state)) ?? 'ok'

// Send limits

export const LIMIT_FIELDS: { name: keyof Limits; label: string }[] = [
  { name: 'per_user_hour', label: 'Each member, an hour' },
  { name: 'per_user_day', label: 'Each member, a day' },
  { name: 'per_ws_hour', label: 'Workspace, an hour' },
  { name: 'per_ws_day', label: 'Workspace, a day' },
  { name: 'max_recipients', label: 'Recipients of one message' },
]

const MAX_LIMIT = 10_000_000

export type LimitTexts = Record<keyof Limits, string>

export const limitTexts = (limits: Limits): LimitTexts => ({
  per_user_hour: String(limits.per_user_hour),
  per_user_day: String(limits.per_user_day),
  per_ws_hour: String(limits.per_ws_hour),
  per_ws_day: String(limits.per_ws_day),
  max_recipients: String(limits.max_recipients),
})

export const limitNumbers = (texts: LimitTexts) => Object.fromEntries(LIMIT_FIELDS.map((field) => [field.name, Number(texts[field.name])])) as unknown as Limits

/** Each number a whole number from 1 to 10,000,000, as the server asks. */
export const limitsValid = (limits: Limits) => LIMIT_FIELDS.every((field) => Number.isInteger(limits[field.name]) && limits[field.name] >= 1 && limits[field.name] <= MAX_LIMIT)
