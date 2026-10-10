import { expect, test } from 'bun:test'
import { intervalText, nextRunText, parseEveryCount } from './recurringLib'

test('the interval text reads naturally for one and for many', () => {
  expect(intervalText(1, 'week', 'schedule')).toBe('Every week')
  expect(intervalText(2, 'week', 'schedule')).toBe('Every 2 weeks')
  expect(intervalText(1, 'month', 'after_completion')).toBe('1 month after completion')
  expect(intervalText(3, 'day', 'after_completion')).toBe('3 days after completion')
})

test('the next run text covers paused, waiting, due and future routines', () => {
  const now = new Date(2026, 9, 8, 12, 0)
  expect(nextRunText({ paused: true, next_run_at: new Date(2026, 9, 9).toISOString() }, now)).toBe('Paused')
  expect(nextRunText({ paused: false, next_run_at: null }, now)).toBe('After the last task is closed')
  expect(nextRunText({ paused: false, next_run_at: new Date(2026, 9, 8, 11, 0).toISOString() }, now)).toBe('Now')
  expect(nextRunText({ paused: false, next_run_at: new Date(2026, 9, 12, 9, 0).toISOString() }, now)).toBe('Oct 12, 9:00 AM')
  expect(nextRunText({ paused: false, next_run_at: new Date(2027, 0, 5, 9, 0).toISOString() }, now)).toBe('Jan 5, 2027, 9:00 AM')
})

test('the interval count is a whole number from 1 to 1000', () => {
  expect(parseEveryCount(' 2 ')).toBe(2)
  expect(parseEveryCount('1000')).toBe(1000)
  expect(parseEveryCount('0')).toBeNull()
  expect(parseEveryCount('1001')).toBeNull()
  expect(parseEveryCount('1.5')).toBeNull()
  expect(parseEveryCount('')).toBeNull()
})
