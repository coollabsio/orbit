import { toast } from 'sonner'
import type { Limits, MailDnsRecordKind, MailDomain, MailProviderKind, MailProviderView, RouteSettings, SendTier } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { fullDate, relativeTime, timeOfDay } from '@/lib/format'
import { writeClipboard } from '@/lib/clipboard'

/** The server's words for a refused mail request (its `detail` names the reason), else `fallback`. */
export function mailProblem(error: unknown, fallback: string): string {
  return error instanceof ApiProblem ? error.detail : `${fallback} Orbit did not answer. Check your connection and try again.`
}

/** Mail is off for this server (Admin → Mail). */
export const isMailDisabled = (error: unknown) => error instanceof ApiProblem && error.code === 'mail_disabled'

/** Copies `text` and says so; `what` names it in the toast ("Name copied"). */
export function copyText(text: string, what: string) {
  void writeClipboard(text).then(
    () => toast(`${what} copied`),
    () => toast.error('Could not copy to the clipboard.'),
  )
}

/** "Fri, Oct 23, 2026, 14:05". */
export const dateTime = (iso: string) => `${fullDate(iso)}, ${timeOfDay(iso)}`

/** "just now", "5m ago", or "on Oct 3" for a week or more. */
export function since(iso: string): string {
  const ago = relativeTime(iso)
  if (ago === 'now') return 'just now'
  return /^\d+[mhd]$/.test(ago) ? `${ago} ago` : `on ${ago}`
}

/** `1 address`, `2 addresses`. */
export const count = (amount: number, one: string, many: string) => `${amount} ${amount === 1 ? one : many}`

/** In the order the form offers them: the recommended one first. */
export const PROVIDER_LABELS: Record<MailProviderKind, string> = {
  postshiba: 'PostShiba',
  cloudflare: 'Cloudflare Email Routing',
  postmark: 'Postmark',
  mailgun: 'Mailgun',
  resend: 'Resend',
  ses: 'Amazon SES',
}

/** The route value for this server's own SMTP listener. */
/** The MX hosts the server uses when the list is empty (mirrors `ProviderKind::default_mx_hosts`). */
export function defaultMxHosts(kind: MailProviderKind, region?: string | null): string[] {
  switch (kind) {
    case 'postshiba':
      return ['inbound.postshiba.com']
    case 'cloudflare':
      return ['route1', 'route2', 'route3'].map((host) => `${host}.mx.cloudflare.net`)
    case 'postmark':
      return ['inbound.postmarkapp.com']
    case 'mailgun':
      return region === 'eu' ? ['mxa.eu.mailgun.org', 'mxb.eu.mailgun.org'] : ['mxa.mailgun.org', 'mxb.mailgun.org']
    case 'ses':
      return [`inbound-smtp.${region || 'us-east-1'}.amazonaws.com`]
    case 'resend':
      return []
  }
}

export const DIRECT = 'direct'

/** "Directly (MX to this server)", then each provider the workspace may use. */
export function routeOptions(providers: MailProviderView[]) {
  return [
    { value: DIRECT, label: 'Directly (MX to this server)' },
    ...providers.map((provider) => ({ value: provider.id, label: `${provider.name} (${PROVIDER_LABELS[provider.kind]})` })),
  ]
}

export const KIND_LABELS: Record<MailDnsRecordKind, string> = {
  verify: 'Verification',
  mx: 'Mail server',
  dkim: 'DKIM',
  spf: 'SPF',
  dmarc: 'DMARC',
}

/** Mail reaches the workspace only with these; SPF and DMARC are for reputation. */
export const REQUIRED: ReadonlySet<MailDnsRecordKind> = new Set(['verify', 'mx'])

/** Orbit signs every sent message with the domain's DKIM key: no mail leaves without its record. */
export const SEND_REQUIRED: ReadonlySet<MailDnsRecordKind> = new Set(['dkim'])

export interface DomainHealth {
  label: string
  variant: 'secondary' | 'outline' | 'destructive'
  hint: string
}

/** One badge for a domain: its status, and for a verified domain whether its DNS records are all in place. */
export function domainHealth(domain: Pick<MailDomain, 'status' | 'records'> & { dkim_rotating?: boolean }): DomainHealth {
  switch (domain.status) {
    case 'pending':
      return { label: 'Pending', variant: 'outline', hint: 'Publish the verification record, then click Check now.' }
    case 'suspended':
      return { label: 'Suspended', variant: 'destructive', hint: 'The verification record is gone. Mail is accepted only during the grace period.' }
    case 'disabled':
      return { label: 'Disabled', variant: 'outline', hint: 'Mail for this domain is refused.' }
    case 'verified': {
      // during a rotation the DKIM record shown is the new key's: the old one still signs
      const bad = domain.records.filter((record) => (record.status.status === 'missing' || record.status.status === 'wrong') && !(record.kind === 'dkim' && domain.dkim_rotating))
      const names = (keep: (kind: MailDnsRecordKind) => boolean) => bad.filter((record) => keep(record.kind)).map((record) => KIND_LABELS[record.kind])
      const required = names((kind) => REQUIRED.has(kind))
      const sending = names((kind) => SEND_REQUIRED.has(kind))
      const recommended = names((kind) => !REQUIRED.has(kind) && !SEND_REQUIRED.has(kind))
      if (required.length > 0) return { label: 'Action needed', variant: 'destructive', hint: `Missing or wrong: ${required.join(', ')}. Mail cannot arrive without it.` }
      if (sending.length > 0 || recommended.length > 0) {
        const parts = [
          sending.length > 0 ? `Mail cannot be sent until ${sending.join(', ')} is in place.` : null,
          recommended.length > 0 ? `Recommended records missing or wrong: ${recommended.join(', ')}.` : null,
        ].filter(Boolean)
        return { label: 'Incomplete', variant: 'outline', hint: `Verified. ${parts.join(' ')}` }
      }
      return { label: 'Healthy', variant: 'secondary', hint: 'Verified, and all DNS records are in place.' }
    }
  }
}

// Sending (slice 2)

/** The send route kinds, in the order the form offers them. */
export const SEND_ROUTE_KINDS = ['direct', 'smtp', 'postshiba', 'postmark', 'resend', 'mailgun', 'ses', 'cloudflare'] as const
export type SendRouteKind = (typeof SEND_ROUTE_KINDS)[number]

export const isSendRouteKind = (kind: string): kind is SendRouteKind => (SEND_ROUTE_KINDS as readonly string[]).includes(kind)

export const ROUTE_LABELS: Record<SendRouteKind, string> = {
  direct: 'Directly from this server',
  smtp: 'SMTP relay',
  postshiba: 'PostShiba',
  postmark: 'Postmark',
  resend: 'Resend',
  mailgun: 'Mailgun',
  ses: 'Amazon SES',
  cloudflare: 'Cloudflare Email Sending (Beta)',
}

/** A route kind's label; an unknown kind (a newer server) shows as it is. */
export const routeLabel = (kind: string) => (isSendRouteKind(kind) ? ROUTE_LABELS[kind] : kind)

/** The kind of route a receiving provider makes with **Also send through this provider**. */
export const routeKindOfProvider = (kind: MailProviderKind): SendRouteKind => kind

/** One input of the send route form: a secret (write only) or a setting of `RouteSettings`. */
export interface FieldSpec {
  group: 'secrets' | 'settings'
  name: string
  label: string
  input: 'text' | 'password' | 'number' | 'select'
  /** Needed to create the route. */
  required: boolean
  placeholder?: string
  /** Help text for the label's info tip. */
  help?: string
  options?: { value: string; label: string }[]
  /** A select's value when the route sets none: what the server uses then. */
  defaultValue?: string
  /** Only on a route of the root user (Admin → Mail). */
  instanceOnly?: boolean
}

const secret = (name: string, label: string, help?: string, required = true): FieldSpec => ({ group: 'secrets', name, label, input: 'password', required, help })
const setting = (name: string, label: string, extra: Partial<FieldSpec> = {}): FieldSpec => ({ group: 'settings', name, label, input: 'text', required: false, ...extra })

const SPF_FIELD = setting('spf', 'SPF entry', {
  placeholder: 'include:spf.example.com',
  help: 'What the domain’s SPF record must allow for this route: an include, IP addresses (ip4:…), or a whole v=spf1 record.',
})
const SMTP_PORT = setting('port', 'Port', { input: 'number', placeholder: '587', help: 'Empty: 465 with TLS, 587 with STARTTLS, 25 without TLS.' })

/** The inputs each kind needs (the server's `RouteSecrets` and `RouteSettings`); `direct` needs none. */
export function routeFields(kind: SendRouteKind): FieldSpec[] {
  switch (kind) {
    case 'direct':
      return []
    case 'smtp':
      return [
        setting('host', 'Host', { required: true, placeholder: 'smtp.example.com' }),
        setting('security', 'Security', {
          input: 'select',
          options: [
            { value: 'starttls', label: 'STARTTLS (port 587)' },
            { value: 'tls', label: 'TLS (port 465)' },
            { value: 'none', label: 'None (private network only)' },
          ],
          defaultValue: 'starttls',
          help: 'Without TLS, only a route of the server admin to a private address is allowed.',
        }),
        SMTP_PORT,
        secret('username', 'User name', 'Leave both empty for a relay without login.', false),
        secret('password', 'Password', undefined, false),
        SPF_FIELD,
      ]
    case 'postshiba':
      return [
        setting('host', 'SMTP host', { required: true, help: 'The SMTP host of your PostShiba cluster.' }),
        SMTP_PORT,
        secret('smtp_username', 'SMTP user name'),
        secret('smtp_password', 'SMTP password'),
        secret('webhook_secret', 'Events webhook secret', 'The signing secret of the events webhook you add for Orbit (not the inbox one).'),
        SPF_FIELD,
      ]
    case 'postmark':
      return [
        secret('server_token', 'Server API token', 'Postmark → Servers → your server → API Tokens.'),
        setting('stream', 'Message stream', { placeholder: 'outbound', help: 'Empty: outbound, the default transactional stream.' }),
      ]
    case 'resend':
      return [
        secret('api_key', 'API key', 'Resend → API keys → Create. Sending access is enough; Full access also works.'),
        secret('webhook_secret', 'Webhook signing secret', 'Resend → Webhooks → add a webhook for email.delivered, email.bounced, email.complained and email.delivery_delayed with the events URL Orbit shows after you add the route. Its signing secret starts with whsec_.'),
      ]
    case 'mailgun':
      return [
        setting('region', 'Region', {
          input: 'select',
          options: [
            { value: 'us', label: 'US' },
            { value: 'eu', label: 'EU' },
            { value: 'au', label: 'AU' },
          ],
          defaultValue: 'us',
        }),
        secret('api_key', 'API key', 'A sending key of the domain, or an account API key.'),
        secret('webhook_signing_key', 'HTTP webhook signing key', 'Mailgun → Settings → API Security → HTTP webhook signing key. One key for the whole account; it is not the API key.'),
        setting('base_url', 'API base URL', { instanceOnly: true, placeholder: 'https://api.mailgun.net', help: 'Empty: the region’s URL. HTTPS only.' }),
      ]
    case 'ses':
      return [
        setting('region', 'AWS region', { required: true, placeholder: 'us-east-1' }),
        secret('access_key_id', 'Access key ID', 'An access key of an IAM user whose policy allows ses:SendEmail and ses:SendRawEmail (AWS advises both).'),
        secret('secret_access_key', 'Secret access key'),
        setting('topic_arn', 'SNS topic ARN', {
          placeholder: 'arn:aws:sns:…',
          help: 'The topic your configuration set publishes bounces, complaints and deliveries to. Without it, Orbit refuses SES events.',
        }),
      ]
    case 'cloudflare':
      return [
        setting('account_id', 'Account ID', { required: true, help: 'The 32-character ID in the Cloudflare dashboard URL.' }),
        secret('api_token', 'API token', 'My Profile → API Tokens → Create token, with the account permission Email Sending: Edit.'),
        SPF_FIELD,
      ]
  }
}

/** The secrets a receiving provider already has, which **Also send through this provider** reuses. */
const PROVIDER_PREFILLED: Partial<Record<SendRouteKind, readonly string[]>> = {
  mailgun: ['webhook_signing_key'],
  ses: ['access_key_id', 'secret_access_key'],
  resend: ['api_key'],
}

/** The select value that leaves a setting to the receiving provider (the server copies the provider's). */
export const PROVIDER_SETTING = 'provider'

/**
 * The inputs of the route form. A workspace route has no instance-only inputs and no plain SMTP (`none` is only for
 * a route of the root user to a private address). `fromProvider`: **Also send through this provider**, which asks
 * only for what the provider lacks; the region is the provider's unless the admin picks one.
 */
export function routeFormFields(kind: SendRouteKind, scope: 'instance' | 'workspace', fromProvider = false): FieldSpec[] {
  const prefilled = fromProvider ? (PROVIDER_PREFILLED[kind] ?? []) : []
  return routeFields(kind)
    .filter((field) => scope === 'instance' || !field.instanceOnly)
    .filter((field) => !(field.group === 'secrets' && prefilled.includes(field.name)))
    .map((field) => {
      if (field.name === 'security' && scope === 'workspace') return { ...field, options: field.options?.filter((option) => option.value !== 'none') }
      if (field.name === 'region' && fromProvider) {
        return field.options
          ? { ...field, required: false, options: [{ value: PROVIDER_SETTING, label: 'The provider’s region' }, ...field.options], defaultValue: PROVIDER_SETTING }
          : { ...field, required: false, placeholder: 'The provider’s region' }
      }
      return field
    })
}

export type FormValues = Record<string, string>

/** The form's starting text of each setting: the saved value, else the select's default. */
export function initialSettingValues(fields: FieldSpec[], settings: RouteSettings | null | undefined): FormValues {
  const values: FormValues = {}
  for (const field of fields.filter((spec) => spec.group === 'settings')) {
    const saved = (settings as Record<string, unknown> | null | undefined)?.[field.name]
    if (saved !== null && saved !== undefined && saved !== '') values[field.name] = String(saved)
    else if (field.defaultValue) values[field.name] = field.defaultValue
  }
  return values
}

/** "Largest message (MB)": empty (the kind's limit) or a positive number of megabytes. */
export function maxBytesValid(text: string): boolean {
  const value = text.trim()
  if (value === '') return true
  const number = Number(value)
  return Number.isFinite(number) && number > 0
}

/**
 * The settings the server gets: `base` (what the form does not show) with the form's values over it. PATCH replaces
 * the settings, so a change sends them all.
 */
export function settingsBody(fields: FieldSpec[], values: FormValues, maxBytes: string, base: RouteSettings | null | undefined): RouteSettings {
  const settings: Record<string, unknown> = { ...(base ?? {}) }
  for (const field of fields.filter((spec) => spec.group === 'settings')) {
    const value = (values[field.name] ?? '').trim()
    if (field.input === 'number') settings[field.name] = value === '' ? null : Number(value)
    else if (field.name === 'security') settings.security = value || 'starttls'
    else settings[field.name] = value === '' || value === PROVIDER_SETTING ? null : value
  }
  const bytes = maxBytes.trim()
  settings.max_message_bytes = bytes === '' ? 0 : Math.round(Number(bytes) * 1_000_000)
  return settings as RouteSettings
}

/** The secrets typed in: empty ones are left out (a change keeps them; the server makes the generated ones). */
export function secretsBody(fields: FieldSpec[], values: FormValues): Record<string, string> {
  const secrets: Record<string, string> = {}
  for (const field of fields.filter((spec) => spec.group === 'secrets')) {
    const value = (values[field.name] ?? '').trim()
    if (value !== '') secrets[field.name] = value
  }
  return secrets
}

/**
 * Whether the route form can be sent: required settings set, required secrets set on a new route, an SMTP login
 * whole on a new route, a port that is a number, and a valid size.
 */
export function routeFormReady(fields: FieldSpec[], form: { settings: FormValues; secrets: FormValues; maxBytes: string; editing: boolean }): boolean {
  const filled = (values: FormValues, name: string) => (values[name] ?? '').trim() !== ''
  for (const field of fields.filter((spec) => spec.required)) {
    if (field.group === 'secrets' && form.editing) continue
    if (!filled(field.group === 'settings' ? form.settings : form.secrets, field.name)) return false
  }
  for (const field of fields.filter((spec) => spec.input === 'number' && filled(form.settings, spec.name))) {
    const number = Number(form.settings[field.name]!.trim())
    if (!Number.isInteger(number) || number < 1 || number > 65_535) return false
  }
  const login = fields.some((field) => field.name === 'username') && fields.some((field) => field.name === 'password')
  if (login && !form.editing && filled(form.secrets, 'username') !== filled(form.secrets, 'password')) return false
  return maxBytesValid(form.maxBytes)
}

/** Secrets the server makes for a new route when none are given (it checks them on the events it receives), shown once. */
export function generatedSecrets(kind: SendRouteKind): string[] {
  if (kind === 'postmark') return ['webhook_user', 'webhook_password']
  if (kind === 'cloudflare') return ['event_secret']
  return []
}

export const SECRET_LABELS: Record<string, string> = {
  webhook_user: 'Webhook user name',
  webhook_password: 'Webhook password',
  event_secret: 'Event secret',
}

/** The size limit of a sent message (bytes, encoded) when the route sets none (spec §5.9). */
export function defaultMaxBytes(kind: SendRouteKind): number {
  switch (kind) {
    case 'postmark':
      return 10_000_000
    case 'cloudflare':
      return 26_214_400
    case 'ses':
      return 40_000_000
    case 'mailgun':
    case 'direct':
    case 'smtp':
    case 'postshiba':
    case 'resend':
      return 25_000_000
  }
}

const KNOWN_SPF: Partial<Record<SendRouteKind, string>> = {
  postmark: 'include:spf.mtasv.net',
  resend: 'include:amazonses.com',
  ses: 'include:amazonses.com',
  mailgun: 'include:mailgun.org',
}

/**
 * What the route adds to the domain's SPF record (as the server's `expected_spf`): Orbit's host for `direct`, the
 * provider's include, else the entry the admin set on the route; `null` while a relay has none.
 */
export function spfHint(kind: SendRouteKind, settings: Pick<RouteSettings, 'spf'> | null | undefined, hostname?: string | null): string | null {
  if (kind === 'direct') return `a:${hostname?.trim() || '<mail hostname>'}`
  const known = KNOWN_SPF[kind]
  if (known) return known
  return settings?.spf?.trim() || null
}

/** "10 MB", "26.2 MB". */
export function megabytes(bytes: number): string {
  const value = bytes / 1_000_000
  return `${Number.isInteger(value) ? value : value.toFixed(1)} MB`
}

export const TIER_LABELS: Record<SendTier, string> = { new: 'New', standard: 'Standard', custom: 'Custom' }

/** The choices of the undo send delay, in seconds. */
export const UNDO_SECONDS = [0, 5, 10, 20, 30] as const

/**
 * What to set up at the provider for a route: its events webhook and the domain's records there. `eventSecretFromProvider`:
 * a Cloudflare route made from a receiving provider, whose Email Worker already has the queue handler and the secret.
 */
export function routeSetupSteps(kind: SendRouteKind, eventSecretFromProvider = false): string[] {
  switch (kind) {
    case 'direct':
      return [
        'Publish the SPF record on each domain’s records page (Settings → Mail → Domains).',
        'Ask your hosting provider to set the PTR record of this server’s sending address to the mail hostname.',
        'Bounces come back by mail to this server.',
      ]
    case 'smtp':
      return [
        'Enter the relay’s SPF entry on the route, so each domain’s SPF record allows it.',
        'Bounces come back by mail, so the domains must receive their mail through Orbit.',
      ]
    case 'postshiba':
      return [
        'In PostShiba, add an events webhook with the URL above, for bounce, complaint and delivery events.',
        'Paste its signing secret into the route (it is not the inbox webhook’s secret).',
        'Publish the records PostShiba shows for each domain: the return path CNAME (rp.<domain>) and its DKIM.',
      ]
    case 'postmark':
      return [
        'In Postmark, open the server’s message stream → Webhooks, and add a webhook with the URL above (it holds the user name and password).',
        'Select Bounce, Spam complaint and Delivery.',
        'Publish the DKIM and Return-Path records Postmark shows for each domain.',
      ]
    case 'resend':
      return [
        'In Resend → Webhooks, add an endpoint with the URL above, for email.bounced, email.complained and email.delivered.',
        'Paste its signing secret (whsec_…) into the route.',
        'Verify each domain at Resend: publish its DKIM and send.<domain> records.',
      ]
    case 'mailgun':
      return [
        'In Mailgun → Sending → Webhooks, add the URL above for Permanent failure, Temporary failure, Spam complaints and Delivered.',
        'Verify each domain at Mailgun: publish the DKIM records it shows.',
      ]
    case 'ses':
      return [
        'Verify each domain as an identity in the route’s region (Easy DKIM).',
        'Make a configuration set with open and click tracking off that publishes Bounce, Complaint and Delivery to an SNS topic, and make it the identities’ default.',
        'Subscribe the URL above (HTTPS) to the topic; Orbit confirms the subscription.',
        'Put the topic’s ARN on the route.',
      ]
    case 'cloudflare':
      if (eventSecretFromProvider) {
        return [
          'Onboard each domain in Cloudflare Email Sending and publish its records (cf-bounce.<domain> and DKIM).',
          'Make a queue, and an event subscription that sends the domains’ sending events to it.',
          'Your Email Worker already has the queue handler, and signs events with its ORBIT_SIGNING_SECRET: bind the queue to it as a consumer. No new secret is needed.',
          'In the Worker’s settings, add the variable ORBIT_EVENTS_URL with the URL above.',
        ]
      }
      return [
        'Onboard each domain in Cloudflare Email Sending and publish its records (cf-bounce.<domain> and DKIM).',
        'Make a queue, and an event subscription that sends the domains’ sending events to it.',
        'Deploy a Worker with the queue handler below, bound to the queue. Set the variable ORBIT_EVENTS_URL to the URL above and the secret ORBIT_EVENT_SECRET to the event secret.',
      ]
  }
}

/** The records the route's provider needs on `domain`, next to Orbit's DKIM and DMARC; `null`: none. */
export function routeRecordsNote(kind: SendRouteKind, domain: string): string | null {
  switch (kind) {
    case 'direct':
      return null
    case 'smtp':
      return 'Also publish the records your relay asks for, if any.'
    case 'postshiba':
      return `Also publish the return path CNAME (rp.${domain}) and the DKIM record PostShiba shows for ${domain}.`
    case 'postmark':
      return `Also publish the DKIM record and the Return-Path CNAME (pm-bounces.${domain}) Postmark shows for ${domain}.`
    case 'resend':
      return `Also publish the DKIM record and the send.${domain} records Resend shows.`
    case 'mailgun':
      return `Also publish the DKIM records Mailgun shows for ${domain}.`
    case 'ses':
      return `Also publish the Easy DKIM records SES shows for ${domain}, and its MAIL FROM records if you set a custom MAIL FROM domain.`
    case 'cloudflare':
      return `Also publish the cf-bounce.${domain} records and the DKIM record Cloudflare shows.`
  }
}

/**
 * The Cloudflare Worker that reads the event queue and posts each batch to Orbit, signed with the event secret (else
 * the Email Worker's signing secret). The same `queue` handler as the Email Worker of a Cloudflare provider
 * (`crates/mail/src/inbound/cloudflare.rs`); the route view carries no script, so a route without that Worker gets
 * this one.
 */
export const CLOUDFLARE_EVENTS_WORKER = `export default {
  async queue(batch, env) {
    const body = JSON.stringify(batch.messages.map((message) => message.body))
    const timestamp = Math.floor(Date.now() / 1000).toString()
    const encoder = new TextEncoder()
    const key = await crypto.subtle.importKey('raw', encoder.encode(env.ORBIT_EVENT_SECRET || env.ORBIT_SIGNING_SECRET), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign'])
    const mac = await crypto.subtle.sign('HMAC', key, encoder.encode(\`\${timestamp}.\${body}\`))
    const signature = [...new Uint8Array(mac)].map((byte) => byte.toString(16).padStart(2, '0')).join('')
    const response = await fetch(env.ORBIT_EVENTS_URL, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'x-orbit-timestamp': timestamp, 'x-orbit-signature': signature },
      body,
    })
    // an error makes the queue deliver the batch again
    if (!response.ok) throw new Error(\`Orbit answered \${response.status}\`)
  },
}
`

/** The events URL to paste at the provider: Postmark's holds its basic auth credentials. */
export function eventsUrlWithLogin(url: string, login: { user: string; password: string } | null): string {
  if (!login) return url
  const [scheme, rest] = url.split('://')
  return rest ? `${scheme}://${encodeURIComponent(login.user)}:${encodeURIComponent(login.password)}@${rest}` : url
}

/**
 * `iso` as the value of a `datetime-local` input in `timeZone` (`2026-11-20T09:00`): the server reads a time without
 * an offset in that zone. `''` without a time.
 */
export function localInputValue(iso: string | null | undefined, timeZone: string): string {
  if (!iso) return ''
  const at = new Date(iso)
  if (Number.isNaN(at.getTime())) return ''
  let parts: Intl.DateTimeFormatPart[]
  try {
    parts = new Intl.DateTimeFormat('en-US', { timeZone, year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }).formatToParts(at)
  } catch {
    return ''
  }
  const part = (type: Intl.DateTimeFormatPartTypes) => parts.find((entry) => entry.type === type)?.value ?? ''
  return `${part('year')}-${part('month')}-${part('day')}T${part('hour')}:${part('minute')}`
}

/** The limits of a tier as rows: what a member and the workspace may send. */
export function limitRows(limits: Limits): { label: string; value: string }[] {
  const number = (value: number) => value.toLocaleString('en-US')
  return [
    { label: 'Each member', value: `${number(limits.per_user_hour)} an hour, ${number(limits.per_user_day)} a day` },
    { label: 'The workspace', value: `${number(limits.per_ws_hour)} an hour, ${number(limits.per_ws_day)} a day` },
    { label: 'Recipients of one message', value: number(limits.max_recipients) },
  ]
}

/** `2.5 %`; under 1 % with two decimals (`0.25 %`), as the complaint limits are that small. */
export function percent(value: number): string {
  if (Number.isInteger(value)) return `${value} %`
  return `${Number(value.toFixed(value < 1 ? 2 : 1))} %`
}
