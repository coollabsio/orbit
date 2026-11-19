import { expect, test } from 'bun:test'
import type { MailDnsRecord, MailDnsRecordKind, MailDomain } from '@orbit/api/generated/types.gen'
import { dnsHost, firstOpenStep, relativeHost, setupSteps, zoneFile } from './domainSetup'

type Status = 'ok' | 'missing' | 'unknown' | 'wrong'

const record = (kind: MailDnsRecordKind, status: Status, extra: Partial<MailDnsRecord> = {}): MailDnsRecord =>
  ({
    kind,
    type: kind === 'mx' ? 'MX' : 'TXT',
    name: kind === 'verify' ? '_orbit-verify.example.com' : 'example.com',
    value: kind,
    txt_strings: [kind],
    priority: kind === 'mx' ? 10 : null,
    status: status === 'wrong' ? { status, found: 'other' } : { status },
    ...extra,
  }) as MailDnsRecord

const domain = (records: MailDnsRecord[], extra: Partial<MailDomain> = {}) =>
  ({ name: 'example.com', records, checked_at: '2026-11-18T10:00:00Z', dkim_rotating: false, provider: null, ...extra }) as MailDomain

const all = (statuses: Partial<Record<MailDnsRecordKind, Status>>) =>
  (['verify', 'mx', 'dkim', 'spf', 'dmarc'] as const).map((kind) => record(kind, statuses[kind] ?? 'ok'))

test('steps go in task order and group SPF with DMARC', () => {
  const steps = setupSteps(domain(all({})))
  expect(steps.map((step) => step.key)).toEqual(['verify', 'receive', 'send', 'inbox'])
  expect(steps[3].records.map((r) => r.kind)).toEqual(['spf', 'dmarc'])
  expect(steps.every((step) => step.state === 'done')).toBe(true)
  expect(steps[3].optional).toBe(true)
  expect(firstOpenStep(steps)).toBeNull()
})

test('the first step that is not done opens; a wrong value is a problem', () => {
  const steps = setupSteps(domain(all({ mx: 'missing', dkim: 'wrong', dmarc: 'missing' })))
  expect(steps.map((step) => step.state)).toEqual(['done', 'todo', 'problem', 'todo'])
  expect(steps[3].summary).toBe('1 of 2 found')
  expect(firstOpenStep(steps)).toBe('receive')
})

test('without an MX record (no mail host name) receiving is blocked, not done', () => {
  const steps = setupSteps(domain(all({}).filter((r) => r.kind !== 'mx')))
  expect(steps[1].state).toBe('blocked')
  expect(steps[1].summary).toBe('Mail host name not set')
})

test('before the first check every step is unknown', () => {
  const steps = setupSteps(domain(all({ verify: 'unknown', mx: 'unknown', dkim: 'unknown', spf: 'unknown', dmarc: 'unknown' }), { checked_at: null }))
  expect(steps.map((step) => step.summary)).toEqual(['Not checked yet', 'Not checked yet', 'Not checked yet', 'Not checked yet'])
})

test('a new DKIM key during a rotation is something to publish, not a problem', () => {
  const steps = setupSteps(domain(all({ dkim: 'wrong' }), { dkim_rotating: true }))
  expect(steps[2].state).toBe('todo')
  expect(steps[2].summary).toBe('Publish the new key')
})

test('record names relative to the zone', () => {
  expect(relativeHost('_orbit-verify.example.com', 'example.com')).toBe('_orbit-verify')
  expect(relativeHost('example.com', 'example.com')).toBe('@')
  expect(relativeHost('o2026a._domainkey.mail.example.com', 'example.com')).toBe('o2026a._domainkey.mail')
  expect(relativeHost('Mail.Example.com.', 'example.com')).toBe('mail')
  // a name outside the zone stays whole; a look-alike suffix is not the zone
  expect(relativeHost('notexample.com', 'example.com')).toBe('notexample.com')
})

test('zone file: TXT strings quoted and split, MX with priority, absolute names', () => {
  const text = zoneFile(domain([
    record('verify', 'ok', { value: 'orbit-verify=abc', txt_strings: ['orbit-verify=abc'] }),
    record('mx', 'ok', { value: 'mx.orbit.example', priority: 10 }),
    record('dkim', 'ok', { name: 'o1._domainkey.example.com', value: 'v=DKIM1; p=AB', txt_strings: ['v=DKIM1; p=A', 'B'] }),
    record('dmarc', 'ok', { name: '_dmarc.example.com', value: 'v=DMARC1; p="none"', txt_strings: ['v=DMARC1; p="none"'] }),
  ]))
  expect(text.split('\n')).toEqual([
    '; Orbit mail records for example.com',
    '_orbit-verify.example.com.\t3600\tIN\tTXT\t"orbit-verify=abc"',
    'example.com.\t3600\tIN\tMX\t10 mx.orbit.example.',
    'o1._domainkey.example.com.\t3600\tIN\tTXT\t"v=DKIM1; p=A" "B"',
    '_dmarc.example.com.\t3600\tIN\tTXT\t"v=DMARC1; p=\\"none\\""',
    '',
  ])
})

test('the DNS host from the name servers', () => {
  expect(dnsHost(['bob.ns.cloudflare.com', 'kim.ns.cloudflare.com'])?.name).toBe('Cloudflare')
  expect(dnsHost(['ns-1234.awsdns-12.org', 'ns-99.awsdns-01.com'])?.name).toBe('Amazon Route 53')
  expect(dnsHost(['dns1.registrar-servers.com'])?.name).toBe('Namecheap')
  expect(dnsHost(['ns-cloud-a1.googledomains.com'])?.name).toBe('Google Cloud DNS')
  // an unknown host: named by its servers' domain, with no link
  expect(dnsHost(['ns1.example.net', 'ns2.example.net'])).toEqual({ name: 'example.net', url: null })
  // a look-alike is not Cloudflare
  expect(dnsHost(['ns.cloudflare.com.evil.net'])?.name).toBe('evil.net')
  expect(dnsHost([])).toBeNull()
})
