import { expect, test } from 'bun:test'

import { deliverySummary, outboxStatus, recipientProblem } from './mail-status'

const recipient = (status: string, delayed = false) => ({ address: `${status}@x.test`, status, delayed })

test('the summary of a sent message', () => {
  expect(deliverySummary([recipient('delivered'), recipient('queued')]).label).toBe('Sending')
  expect(deliverySummary([recipient('delivered'), recipient('deferred', true)]).label).toBe('Delayed')
  expect(deliverySummary([recipient('delivered'), recipient('cancelled')]).label).toBe('Delivered')
  expect(deliverySummary([recipient('delivered'), recipient('bounced')])).toEqual({ label: 'Partly delivered', state: 'bounced', failed: 1 })
  expect(deliverySummary([recipient('suppressed')])).toEqual({ label: 'Suppressed', state: 'suppressed', failed: 1 })
})

test('why a recipient did not get it', () => {
  expect(recipientProblem({ status: 'bounced', delayed: false, bounce_kind: 'rejected', last_response: ' Over quota ' })).toBe('Over quota')
  expect(recipientProblem({ status: 'bounced', delayed: false, bounce_kind: 'gave_up' })).toBe('Gave up after 5 days')
  expect(recipientProblem({ status: 'delivered', delayed: false })).toBe(null)
})

test('an Outbox row says why it waits or when it goes', () => {
  const now = new Date('2026-11-20T10:00:00Z')
  expect(outboxStatus({ status: 'held', release_at: '2026-11-20T10:00:20Z' }, now)).toEqual({ text: 'Sending in 20 s', problem: false, canCancel: true })
  expect(outboxStatus({ status: 'held', release_at: '2026-11-20T10:05:00Z' }, now).text).toBe('Sending in 5 min')
  expect(outboxStatus({ status: 'held', hold_reason: 'paused', release_at: '2026-11-20T09:00:00Z' }, now)).toEqual({ text: 'Sending is paused for this workspace', problem: true, canCancel: true })
  expect(outboxStatus({ status: 'released', release_at: '2026-11-20T10:00:20Z' }, now)).toEqual({ text: 'Sending', problem: false, canCancel: false })
})
