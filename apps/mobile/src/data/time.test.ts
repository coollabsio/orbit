import { createOrbitClient } from '@orbit/api/client'
import { describe, expect, test } from 'bun:test'

import { createTimeEntry, dayDate, deleteTimeEntry, formatDuration, parseDuration, takeSpend, timeTotalLabel, todayLocal, updateTimeEntry, workedOnLabel } from './time'

test('a duration reads hours, minutes, both, decimals and bare minutes, in any case and spacing', () => {
  expect(parseDuration('1h 30m')).toBe(90)
  expect(parseDuration('1h30m')).toBe(90)
  expect(parseDuration('90m')).toBe(90)
  expect(parseDuration('1.5h')).toBe(90)
  expect(parseDuration('2h')).toBe(120)
  expect(parseDuration('45')).toBe(45)
  expect(parseDuration(' 1 H  30 M ')).toBe(90)
  expect(parseDuration('24h')).toBe(1440)
  expect(parseDuration('.5h')).toBe(30)
})

test('a duration refuses days, weeks, zero, negatives, more than a day and garbage', () => {
  for (const text of ['1d', '1w', '2h 1d', '0', '0h', '0m', '-30', '-1h', '24h 1m', '1441', '', '   ', 'h', 'm', 'abc', '1h30', '30m 1h', '1.5m', '1 30', '1 5', '2 4h']) {
    expect([text, parseDuration(text)]).toEqual([text, null])
  }
})

test('a duration prints as hours and minutes', () => {
  expect([formatDuration(90), formatDuration(45), formatDuration(120), formatDuration(1440)]).toEqual(['1h 30m', '45m', '2h', '24h'])
  expect(formatDuration(parseDuration('1h 30m')!)).toBe('1h 30m')
})

test('today and a worked-on day are local days', () => {
  expect(todayLocal(new Date(2026, 0, 5, 23, 59))).toBe('2026-01-05')
  const day = dayDate('2026-11-10')
  expect([day.getFullYear(), day.getMonth(), day.getDate()]).toEqual([2026, 10, 10])
})

test('a worked-on day shows its year only when it is not this year', () => {
  const now = new Date(2026, 10, 24)
  expect(workedOnLabel('2026-11-10', now)).toBe(new Date(2026, 10, 10).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }))
  expect(workedOnLabel('2025-12-31', now)).toContain('2025')
})

test('the Time property adds the sub-issues only when they logged time', () => {
  expect(timeTotalLabel({ total_minutes: 90, sub_issues_minutes: 0 })).toEqual({ own: '1h 30m', subIssues: null })
  expect(timeTotalLabel({ total_minutes: 0, sub_issues_minutes: 120 })).toEqual({ own: '0m', subIssues: '+2h sub-issues' })
})

function server(reply: { status: number; body?: unknown }) {
  const sent: { method: string; url: string; body: unknown }[] = []
  const client = createOrbitClient({
    baseUrl: 'https://orbit.test',
    token: () => 'session-token',
    fetch: async (request) => {
      const text = await request.text()
      sent.push({ method: request.method, url: request.url, body: text ? JSON.parse(text) : undefined })
      return reply.body === undefined ? new Response(null, { status: reply.status }) : new Response(JSON.stringify(reply.body), { status: reply.status, headers: { 'content-type': 'application/json' } })
    },
  })
  return { client, sent }
}

test('a new entry leaves out an empty note and the user unless one is chosen; an edit sends the version', async () => {
  const create = server({ status: 201, body: { id: 'e1' } })
  await createTimeEntry(create.client, 'w1', 't1', { minutes: 90, worked_on: '2026-11-10', note: '  ', billable: true })
  await createTimeEntry(create.client, 'w1', 't1', { minutes: 30, worked_on: '2026-11-10', note: ' Review ', billable: false, user_id: 'u2' })
  expect(create.sent).toEqual([
    { method: 'POST', url: 'https://orbit.test/api/v1/workspaces/w1/tasks/t1/time-entries', body: { minutes: 90, worked_on: '2026-11-10', billable: true } },
    { method: 'POST', url: 'https://orbit.test/api/v1/workspaces/w1/tasks/t1/time-entries', body: { minutes: 30, worked_on: '2026-11-10', billable: false, note: 'Review', user_id: 'u2' } },
  ])
  const edit = server({ status: 200, body: { id: 'e1' } })
  await updateTimeEntry(edit.client, 'w1', { id: 'e1', version: 3 }, { minutes: 60, worked_on: '2026-11-09', note: '', billable: false })
  expect(edit.sent).toEqual([{ method: 'PATCH', url: 'https://orbit.test/api/v1/workspaces/w1/time-entries/e1', body: { minutes: 60, worked_on: '2026-11-09', note: '', billable: false, expected_version: 3 } }])
  const remove = server({ status: 204 })
  await deleteTimeEntry(remove.client, 'w1', 'e1')
  expect(remove.sent).toEqual([{ method: 'DELETE', url: 'https://orbit.test/api/v1/workspaces/w1/time-entries/e1', body: undefined }])
})

describe('takeSpend', () => {
  test('takes the /spend line out of the Markdown and keeps the rest', () => {
    expect(takeSpend('Fixed the retry\n/spend 1h 30m')).toEqual({ body: 'Fixed the retry', spend: { minutes: 90 } })
    expect(takeSpend('/SPEND 2h 2026-11-10')).toEqual({ body: '', spend: { minutes: 120, workedOn: '2026-11-10' } })
  })

  test('leaves a comment without a /spend line alone', () => {
    expect(takeSpend('see /spend 1h')).toEqual({ body: 'see /spend 1h', spend: null })
  })

  test('leaves a /spend line in a fenced code block alone', () => {
    const body = 'Use:\n```\n/spend 1h\n```'
    expect(takeSpend(body)).toEqual({ body, spend: null })
  })

  test('refuses a /spend line it cannot read', () => {
    expect(() => takeSpend('/spend 3d')).toThrow('Could not read')
  })
})
