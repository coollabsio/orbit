import { expect, test } from 'bun:test'
import type { MailDnsRecord, MailDnsRecordKind } from '@orbit/api/generated/types.gen'
import { defaultMaxBytes, domainHealth, generatedSecrets, limitRows, localInputValue, percent, providerRouteFields, TIER_LIMITS, routeFields, SEND_ROUTE_KINDS, type SendRouteKind, spfHint } from './mailSettingsLib'

const record = (kind: MailDnsRecordKind, status: 'ok' | 'missing' | 'unknown'): MailDnsRecord =>
  ({ kind, type: 'TXT', name: kind, value: kind, txt_strings: [], priority: null, status: { status } }) as MailDnsRecord

const records = (statuses: Partial<Record<MailDnsRecordKind, 'ok' | 'missing' | 'unknown'>>) =>
  (['verify', 'mx', 'dkim', 'spf', 'dmarc'] as const).map((kind) => record(kind, statuses[kind] ?? 'ok'))

test('a verified domain without its MX record needs action, even when verification is OK', () => {
  const health = domainHealth({ status: 'verified', records: records({ mx: 'missing', dkim: 'missing' }) })
  expect(health).toEqual({ label: 'Action needed', variant: 'destructive', hint: 'Missing or wrong: Mail server. Mail cannot arrive without it.' })
})

test('a verified domain without DKIM or recommended records is incomplete: it receives but cannot send', () => {
  const health = domainHealth({ status: 'verified', records: records({ dkim: 'missing', spf: 'missing', dmarc: 'missing' }) })
  expect(health.label).toBe('Incomplete')
  expect(health.hint).toBe('Verified. Mail cannot be sent until DKIM is in place. Recommended records missing or wrong: SPF, DMARC.')
  expect(domainHealth({ status: 'verified', records: records({ spf: 'missing' }) }).hint).toBe('Verified. Recommended records missing or wrong: SPF.')
})

test('a verified domain is healthy when every record is OK; an unanswered lookup is not a failure', () => {
  expect(domainHealth({ status: 'verified', records: records({ spf: 'unknown' }) }).label).toBe('Healthy')
})

test('other statuses show as they are, whatever the records say', () => {
  expect(domainHealth({ status: 'pending', records: records({}) }).label).toBe('Pending')
  expect(domainHealth({ status: 'suspended', records: records({}) }).label).toBe('Suspended')
  expect(domainHealth({ status: 'disabled', records: records({ mx: 'missing' }) }).label).toBe('Disabled')
})

test('route_fields_per_kind: each kind asks for its own secrets and settings', () => {
  const names = (kind: SendRouteKind) => routeFields(kind).map((field) => `${field.group}.${field.name}`)
  expect(names('direct')).toEqual([])
  expect(names('smtp')).toEqual(['settings.host', 'settings.security', 'settings.port', 'secrets.username', 'secrets.password', 'settings.spf'])
  expect(names('postshiba')).toEqual(['settings.host', 'settings.port', 'secrets.smtp_username', 'secrets.smtp_password', 'secrets.webhook_secret', 'settings.spf'])
  expect(names('postmark')).toEqual(['secrets.server_token', 'settings.stream'])
  expect(names('resend')).toEqual(['secrets.api_key', 'secrets.webhook_secret'])
  expect(names('mailgun')).toEqual(['settings.region', 'secrets.api_key', 'secrets.webhook_signing_key', 'settings.base_url'])
  expect(names('ses')).toEqual(['settings.region', 'secrets.access_key_id', 'secrets.secret_access_key', 'settings.topic_arn'])
  expect(names('cloudflare')).toEqual(['settings.account_id', 'secrets.api_token', 'settings.spf'])
  // what is required to create the route; an SMTP login is optional (both or neither)
  const required = (kind: SendRouteKind) => routeFields(kind).filter((field) => field.required).map((field) => field.name)
  expect(required('smtp')).toEqual(['host'])
  expect(required('postshiba')).toEqual(['host', 'smtp_username', 'smtp_password', 'webhook_secret'])
  expect(required('ses')).toEqual(['region', 'access_key_id', 'secret_access_key'])
  expect(required('cloudflare')).toEqual(['account_id', 'api_token'])
  // secrets are never shown: password inputs
  for (const kind of SEND_ROUTE_KINDS) {
    for (const field of routeFields(kind).filter((spec) => spec.group === 'secrets')) expect(field.input).toBe('password')
  }
  // only the root user may point Mailgun at another API base URL
  expect(routeFields('mailgun').find((field) => field.name === 'base_url')?.instanceOnly).toBe(true)
  expect(routeFields('mailgun').find((field) => field.name === 'region')?.options?.map((option) => option.value)).toEqual(['us', 'eu', 'au'])
  expect(routeFields('smtp').find((field) => field.name === 'security')?.options?.map((option) => option.value)).toEqual(['starttls', 'tls', 'none'])
})

test('route_fields_per_kind: Orbit makes the webhook secrets that it checks itself', () => {
  expect(generatedSecrets('postmark')).toEqual(['webhook_user', 'webhook_password'])
  expect(generatedSecrets('cloudflare')).toEqual(['event_secret'])
  for (const kind of ['direct', 'smtp', 'postshiba', 'resend', 'mailgun', 'ses'] as const) expect(generatedSecrets(kind)).toEqual([])
})

test('default_max_bytes: the size limit of each kind', () => {
  expect(defaultMaxBytes('postmark')).toBe(10_000_000)
  expect(defaultMaxBytes('mailgun')).toBe(25_000_000)
  expect(defaultMaxBytes('cloudflare')).toBe(26_214_400)
  expect(defaultMaxBytes('ses')).toBe(40_000_000)
  for (const kind of ['direct', 'smtp', 'postshiba', 'resend'] as const) expect(defaultMaxBytes(kind)).toBe(25_000_000)
})

test('spf_hint_per_kind: what the route adds to the domain SPF record', () => {
  expect(spfHint('direct', null, 'mx.orbit.example')).toBe('a:mx.orbit.example')
  expect(spfHint('postmark', null)).toBe('include:spf.mtasv.net')
  expect(spfHint('resend', null)).toBe('include:amazonses.com')
  expect(spfHint('ses', null)).toBe('include:amazonses.com')
  expect(spfHint('mailgun', null)).toBe('include:mailgun.org')
  // a known include wins over the route's own entry, as on the server
  expect(spfHint('postmark', { spf: 'include:other.example' })).toBe('include:spf.mtasv.net')
  // a relay: what the admin entered, else nothing yet
  expect(spfHint('smtp', { spf: ' include:relay.example ' })).toBe('include:relay.example')
  expect(spfHint('smtp', { spf: 'v=spf1 ip4:192.0.2.1 -all' })).toBe('v=spf1 ip4:192.0.2.1 -all')
  expect(spfHint('postshiba', { spf: '' })).toBeNull()
  expect(spfHint('cloudflare', undefined)).toBeNull()
})

test('route_fields_per_kind: a route made from a provider asks only for what the provider lacks', () => {
  const names = (kind: SendRouteKind) => providerRouteFields(kind).map((field) => field.name)
  expect(names('mailgun')).toEqual(['region', 'api_key', 'base_url'])
  expect(names('ses')).toEqual(['region', 'topic_arn'])
  expect(names('resend')).toEqual(['webhook_secret'])
  expect(names('postmark')).toEqual(['server_token', 'stream'])
  expect(names('postshiba')).toEqual(['host', 'port', 'smtp_username', 'smtp_password', 'webhook_secret', 'spf'])
  // the region comes from the provider when left empty
  expect(providerRouteFields('ses').find((field) => field.name === 'region')?.required).toBe(false)
})

test('a vacation time shows in the time zone the server reads it in', () => {
  expect(localInputValue('2026-11-20T08:00:00Z', 'Europe/Berlin')).toBe('2026-11-20T09:00')
  expect(localInputValue('2026-11-20T23:30:00Z', 'America/New_York')).toBe('2026-11-20T18:30')
  expect(localInputValue('2026-11-20T00:00:00Z', 'UTC')).toBe('2026-11-20T00:00')
  expect(localInputValue(null, 'UTC')).toBe('')
  expect(localInputValue('not a date', 'UTC')).toBe('')
})

test('tier limits and rates read plainly', () => {
  expect(limitRows(TIER_LIMITS.new)).toEqual([
    { label: 'Each member', value: '30 an hour, 100 a day' },
    { label: 'The workspace', value: '60 an hour, 200 a day' },
    { label: 'Recipients of one message', value: '50' },
  ])
  expect(limitRows(TIER_LIMITS.standard)[1]!.value).toBe('2,000 an hour, 10,000 a day')
  expect(percent(0)).toBe('0 %')
  expect(percent(5.25)).toBe('5.3 %')
  expect(percent(0.25)).toBe('0.25 %')
  expect(percent(0.1)).toBe('0.1 %')
})
