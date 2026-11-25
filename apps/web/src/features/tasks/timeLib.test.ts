import { describe, expect, test } from 'bun:test'
import { formatDuration, parseDuration, takeSpend, todayLocalDate, workedOnLabel } from './timeLib'

test('parses hours, minutes and bare numbers as minutes', () => {
  expect(parseDuration('1h 30m')).toBe(90)
  expect(parseDuration('1h30m')).toBe(90)
  expect(parseDuration('90m')).toBe(90)
  expect(parseDuration('1.5h')).toBe(90)
  expect(parseDuration('2h')).toBe(120)
  expect(parseDuration('45')).toBe(45)
  expect(parseDuration(' 1H 5M ')).toBe(65)
  expect(parseDuration('1 h 30 m')).toBe(90)
  expect(parseDuration('.5h')).toBe(30)
  expect(parseDuration('24h')).toBe(1440)
})

test('refuses other units, zero, negatives, more than a day and garbage', () => {
  for (const text of ['', '1d', '1w', '2d 3h', '0', '0m', '0h', '-30m', '-5', '1441', '24h 1m', '25h', 'abc', '1h30', '1.5', '30m 1h', 'h', 'm', '1x']) {
    expect(parseDuration(text)).toBeNull()
  }
})

test('formats minutes as hours and minutes', () => {
  expect(formatDuration(90)).toBe('1h 30m')
  expect(formatDuration(45)).toBe('45m')
  expect(formatDuration(120)).toBe('2h')
  expect(formatDuration(0)).toBe('0m')
  expect(formatDuration(1505)).toBe('25h 5m')
})

test('gives the local date, not the UTC one', () => {
  // 23:30 local time on Nov 24: the date stays Nov 24 whatever the UTC date is
  expect(todayLocalDate(new Date(2026, 10, 24, 23, 30))).toBe('2026-11-24')
  expect(todayLocalDate(new Date(2026, 0, 5, 0, 10))).toBe('2026-01-05')
})

test('labels a work day on its local calendar day', () => {
  const now = new Date(2026, 10, 24)
  expect(workedOnLabel('2026-11-10', now)).toBe(new Date(2026, 10, 10).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }))
  expect(workedOnLabel('2025-12-31', now)).toContain('2025')
})

describe('takeSpend', () => {
  const line = (text: string) => ({ type: 'paragraph', props: {}, content: [{ type: 'text', text, styles: {} }], children: [] })

  test('takes the /spend line out and keeps the rest', () => {
    const rest = line('Fixed the retry')
    expect(takeSpend([rest, line('/spend 1h 30m')])).toEqual({ blocks: [rest], spend: { minutes: 90 } })
    expect(takeSpend([line('/SPEND 2h 2026-11-10')])).toEqual({ blocks: [], spend: { minutes: 120, workedOn: '2026-11-10' } })
  })

  test('leaves a comment without a /spend line alone', () => {
    const blocks = [line('spend 1h on it'), line('see /spend 1h')]
    expect(takeSpend(blocks)).toEqual({ blocks, spend: null })
  })

  test('leaves a /spend line in a code block alone', () => {
    const blocks = [{ ...line('/spend 1h'), type: 'codeBlock' }]
    expect(takeSpend(blocks)).toEqual({ blocks, spend: null })
  })

  test('refuses a /spend line it cannot read', () => {
    expect(() => takeSpend([line('/spend 3d')])).toThrow('Could not read')
    expect(() => takeSpend([line('/spend 25h')])).toThrow('Could not read')
  })
})
