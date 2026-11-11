import { expect, test } from 'bun:test'
import type { MailDnsRecord, MailDnsRecordKind } from '@orbit/api/generated/types.gen'
import { domainHealth } from './mailSettingsLib'

const record = (kind: MailDnsRecordKind, status: 'ok' | 'missing' | 'unknown'): MailDnsRecord =>
  ({ kind, type: 'TXT', name: kind, value: kind, txt_strings: [], priority: null, status: { status } }) as MailDnsRecord

const records = (statuses: Partial<Record<MailDnsRecordKind, 'ok' | 'missing' | 'unknown'>>) =>
  (['verify', 'mx', 'dkim', 'spf', 'dmarc'] as const).map((kind) => record(kind, statuses[kind] ?? 'ok'))

test('a verified domain without its MX record needs action, even when verification is OK', () => {
  const health = domainHealth({ status: 'verified', records: records({ mx: 'missing', dkim: 'missing' }) })
  expect(health).toEqual({ label: 'Action needed', variant: 'destructive', hint: 'Missing or wrong: Mail server. Mail cannot arrive without it.' })
})

test('a verified domain that lacks only recommended records is incomplete', () => {
  const health = domainHealth({ status: 'verified', records: records({ dkim: 'missing', spf: 'missing', dmarc: 'missing' }) })
  expect(health.label).toBe('Incomplete')
  expect(health.hint).toBe('Verified. Recommended records missing or wrong: DKIM, SPF, DMARC.')
})

test('a verified domain is healthy when every record is OK; an unanswered lookup is not a failure', () => {
  expect(domainHealth({ status: 'verified', records: records({ spf: 'unknown' }) }).label).toBe('Healthy')
})

test('other statuses show as they are, whatever the records say', () => {
  expect(domainHealth({ status: 'pending', records: records({}) }).label).toBe('Pending')
  expect(domainHealth({ status: 'suspended', records: records({}) }).label).toBe('Suspended')
  expect(domainHealth({ status: 'disabled', records: records({ mx: 'missing' }) }).label).toBe('Disabled')
})
