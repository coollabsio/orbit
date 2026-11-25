import { expect, test } from 'bun:test'
import { formatDuration, parseDuration } from './duration'

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
  for (const text of ['', '1d', '1w', '2d 3h', '0', '0m', '0h', '-30m', '-5', '1441', '24h 1m', '25h', 'abc', '1h30', '1.5', '30m 1h', 'h', 'm', '1x', '   ', '2h 1d', '-1h', '1.5m', '1 30', '2 4h']) {
    expect([text, parseDuration(text)]).toEqual([text, null])
  }
})

test('formats minutes as hours and minutes', () => {
  expect(formatDuration(90)).toBe('1h 30m')
  expect(formatDuration(45)).toBe('45m')
  expect(formatDuration(120)).toBe('2h')
  expect(formatDuration(0)).toBe('0m')
  expect(formatDuration(1505)).toBe('25h 5m')
  expect(formatDuration(parseDuration('1h 30m')!)).toBe('1h 30m')
})
