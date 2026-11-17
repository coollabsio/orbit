import { expect, test } from 'bun:test'
import { ApiProblem } from '@orbit/api/problem'
import { HOLD_REASONS, bounceReason, deliverySummary, heldReplyState, mayChangeVacation, outboxStatus, recipientState, recipientTag, retryErrorText, vacationState } from './mailStatus'

const recipient = (address: string, status: string, delayed = false) => ({ address, status, delayed })

test('summary_all_delivered', () => {
  expect(deliverySummary([recipient('a@x.test', 'delivered'), recipient('b@x.test', 'delivered')])).toEqual({ label: 'Delivered', bounced: [] })
  // a cancelled recipient never goes: it does not count
  expect(deliverySummary([recipient('a@x.test', 'delivered'), recipient('c@x.test', 'cancelled')]).label).toBe('Delivered')
})

test('summary_partly', () => {
  expect(deliverySummary([recipient('a@x.test', 'delivered'), recipient('b@x.test', 'bounced')])).toEqual({ label: 'Partly delivered', bounced: ['b@x.test'] })
  expect(deliverySummary([recipient('a@x.test', 'delivered'), recipient('b@x.test', 'suppressed')]).label).toBe('Partly delivered')
  // one still on its way: the message is still sending
  expect(deliverySummary([recipient('a@x.test', 'delivered'), recipient('b@x.test', 'queued')]).label).toBe('Sending')
  expect(deliverySummary([recipient('a@x.test', 'delivered'), recipient('b@x.test', 'sending')]).label).toBe('Sending')
  expect(deliverySummary([]).label).toBe('Sending')
})

test('delayed_after_4h', () => {
  // the server sets `delayed` once a deferred recipient's first attempt is more than 4 hours ago
  expect(deliverySummary([recipient('a@x.test', 'deferred', false)]).label).toBe('Sending')
  expect(deliverySummary([recipient('a@x.test', 'deferred', true)]).label).toBe('Delayed')
  expect(deliverySummary([recipient('a@x.test', 'delivered'), recipient('b@x.test', 'deferred', true)]).label).toBe('Delayed')
  expect(recipientState(recipient('a@x.test', 'deferred', false))).toBe('sending')
  expect(recipientState(recipient('a@x.test', 'deferred', true))).toBe('delayed')
})

test('bounced_lists_addresses', () => {
  const summary = deliverySummary([recipient('a@x.test', 'bounced'), recipient('b@x.test', 'delivered'), recipient('c@x.test', 'bounced')])
  expect(summary.bounced).toEqual(['a@x.test', 'c@x.test'])
  expect(deliverySummary([recipient('a@x.test', 'bounced'), recipient('c@x.test', 'bounced')])).toEqual({ label: 'Bounced', bounced: ['a@x.test', 'c@x.test'] })
  // none delivered: a bounce weighs more than a suppression
  expect(deliverySummary([recipient('a@x.test', 'bounced'), recipient('b@x.test', 'suppressed')]).label).toBe('Bounced')
  expect(deliverySummary([recipient('b@x.test', 'suppressed')])).toEqual({ label: 'Suppressed', bounced: [] })
})

test('a bounce reason in plain words for each kind', () => {
  expect(bounceReason('hard', '550 5.1.1 no such user')).toBe('The address does not exist or refused the message')
  expect(bounceReason('gave_up', null)).toBe('Gave up after 5 days')
  expect(bounceReason('route', 'Route error: 401')).toBe('The sending route has a problem')
  expect(bounceReason('unreadable', null)).toBe('Orbit could not read the sent message')
  // Orbit refused it itself: its own text is the reason
  expect(bounceReason('rejected', '  5.3.4 The message is larger than the route allows  ')).toBe('5.3.4 The message is larger than the route allows')
  expect(bounceReason('rejected', null)).toBe('Orbit refused to send it')
  expect(bounceReason(null, null)).toBe('The message was not delivered')
})

test('an outbox row says when it goes and why it waits', () => {
  const now = new Date('2026-11-16T10:00:00Z')
  const item = { status: 'held', hold_reason: null, release_at: '2026-11-16T10:00:08Z', scheduled: false }
  expect(outboxStatus(item, now)).toEqual({ text: 'Sending in 8 s', problem: false, canCancel: true })
  expect(outboxStatus({ ...item, release_at: '2026-11-16T09:59:00Z' }, now)).toEqual({ text: 'Sending', problem: false, canCancel: true })
  expect(outboxStatus({ ...item, status: 'released' }, now)).toEqual({ text: 'Sending', problem: false, canCancel: false })
  expect(outboxStatus({ ...item, hold_reason: 'rate_limit' }, now).text).toBe('Over the send limit: it goes when the limit allows')
  expect(outboxStatus({ ...item, hold_reason: 'limit', status: 'released' }, now)).toEqual({
    text: 'Over the send limit: it goes when the limit allows',
    problem: false,
    canCancel: false,
  })
  expect(outboxStatus({ ...item, hold_reason: 'paused' }, now)).toEqual({ text: 'Sending is paused for this workspace', problem: true, canCancel: true })
  expect(outboxStatus({ ...item, hold_reason: 'dkim_inactive' }, now).problem).toBe(true)
  expect(outboxStatus({ ...item, hold_reason: 'something_new' }, now).text).toBe('Waiting: something went wrong')
})

test('a vacation reply is on between its start and its end', () => {
  const now = new Date('2026-11-16T10:00:00Z')
  const vacation = { enabled: true, starts_at: null, ends_at: null }
  expect(vacationState(vacation, now)).toBe('on')
  expect(vacationState({ ...vacation, enabled: false }, now)).toBe('off')
  expect(vacationState({ ...vacation, starts_at: '2026-11-16T10:00:00Z' }, now)).toBe('on')
  expect(vacationState({ ...vacation, starts_at: '2026-11-17T00:00:00Z' }, now)).toBe('scheduled')
  // the end is exclusive
  expect(vacationState({ ...vacation, ends_at: '2026-11-16T10:00:00Z' }, now)).toBe('off')
  expect(vacationState({ ...vacation, ends_at: '2026-11-20T00:00:00Z' }, now)).toBe('on')
})

test('a failed Retry says why: too old, or the server text', () => {
  const problem = (status: number, code: string, detail: string) =>
    new ApiProblem({ type: 'about:blank', title: 'x', status, code, detail, instance: '/x', request_id: 'r' })
  expect(retryErrorText(problem(409, 'recipient_expired', 'Gone.'))).toBe('This message is too old to retry')
  expect(retryErrorText(problem(409, 'not_retryable', 'Only a bounced recipient can be sent again.'))).toBe('Only a bounced recipient can be sent again.')
  expect(retryErrorText(new Error('network'))).toBe('The message could not be sent again.')
})

test('a Bcc recipient is labelled Bcc; To and Cc are not', () => {
  expect(recipientTag('bcc')).toBe('Bcc')
  expect(recipientTag('to')).toBeNull()
  expect(recipientTag('cc')).toBeNull()
})

test('Turn off: the owner of a personal mailbox, or a mail manager for a shared one', () => {
  expect(mayChangeVacation('personal', { owner: true, canManage: false })).toBe(true)
  // a manager cannot change someone else's personal vacation
  expect(mayChangeVacation('personal', { owner: false, canManage: true })).toBe(false)
  expect(mayChangeVacation('shared', { owner: false, canManage: true })).toBe(true)
  expect(mayChangeVacation('shared', { owner: false, canManage: false })).toBe(false)
})

test('a held reply stops showing once lists read after it no longer hold it', () => {
  const held = { id: 'ob1', status: 'held' }
  const since = 1000
  // a list read before the reply was found held says nothing
  expect(heldReplyState('ob1', since, [{ items: [], updatedAt: 900 }])).toBe('unknown')
  expect(heldReplyState('ob1', since, [{ items: [held], updatedAt: 1100 }, { items: [], updatedAt: 1100 }])).toBe('held')
  // released (going) or gone from both lists: no longer held
  expect(heldReplyState('ob1', since, [{ items: [{ id: 'ob1', status: 'released' }], updatedAt: 1100 }, { items: [], updatedAt: 1100 }])).toBe('gone')
  expect(heldReplyState('ob1', since, [{ items: [], updatedAt: 1100 }, { items: [], updatedAt: 1200 }])).toBe('gone')
  // one list still old: wait for it
  expect(heldReplyState('ob1', since, [{ items: [], updatedAt: 1100 }, { items: [], updatedAt: 0 }])).toBe('unknown')
})

test('every hold reason the server writes has its own text', () => {
  const server = [
    'rate_limit', 'limit', 'paused', 'dkim_inactive', 'domain_unverified', 'inbound_not_live', 'no_route',
    'mailbox_suspended', 'file_missing', 'link_origin_missing', 'link_forwarded', 'link_inline', 'send_as_denied',
    'invalid_address', 'no_recipients', 'draft_missing', 'build_failed', 'decrypt_failed', 'storage_quota',
    'release_failed', 'release_error',
  ]
  const now = new Date('2026-11-16T10:00:00Z')
  for (const reason of server) {
    expect(HOLD_REASONS[reason]?.text).toBeTruthy()
    const status = outboxStatus({ status: 'held', hold_reason: reason, release_at: '2026-11-16T10:00:00Z' }, now)
    expect(status.text).not.toBe('Waiting: something went wrong')
  }
  expect(outboxStatus({ status: 'held', hold_reason: 'mailbox_suspended', release_at: '2026-11-16T10:00:00Z' }, now)).toEqual({
    text: 'The mailbox is suspended. It goes once it is active again',
    problem: true,
    canCancel: true,
  })
})
