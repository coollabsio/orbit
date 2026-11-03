import { describe, expect, test } from 'bun:test'

import { dayLabel, fullDate, relativeAgo, relativeTime, shortDate, timeOfDay } from './format'

const at = (day: number, hour: number, minute = 0) => new Date(2026, 9, day, hour, minute).toISOString()
const now = new Date(2026, 9, 23, 20, 5)

describe('dates', () => {
  test('short and full dates', () => {
    expect(shortDate(at(15, 21, 52), 'en-US')).toBe('Oct 15')
    expect(fullDate(at(15, 21, 52), 'en-US')).toBe('Thu, Oct 15, 2026')
  })

  test('time of day is two-digit with the period in en-US', () => {
    expect(timeOfDay(at(15, 21, 52), 'en-US')).toBe('09:52 PM')
    expect(timeOfDay(at(15, 9, 5), 'en-GB')).toBe('09:05')
  })

  test('the day pill', () => {
    expect(dayLabel(at(23, 8), now, 'en-US')).toBe('Today')
    expect(dayLabel(at(22, 23), now, 'en-US')).toBe('Yesterday')
    expect(dayLabel(at(15, 21, 52), now, 'en-US')).toBe('Thu, Oct 15, 2026')
  })
})

describe('relative', () => {
  test('steps from now to days, then the date', () => {
    expect(relativeTime(at(23, 20, 5), now, 'en-US')).toBe('now')
    expect(relativeTime(at(23, 19, 50), now, 'en-US')).toBe('15m')
    expect(relativeTime(at(23, 17), now, 'en-US')).toBe('3h')
    expect(relativeTime(at(21, 20), now, 'en-US')).toBe('2d')
    expect(relativeTime(at(15, 21, 52), now, 'en-US')).toBe('Oct 15')
  })

  test('ago wording for a last reply', () => {
    expect(relativeAgo(at(23, 20, 5), now, 'en-US')).toBe('just now')
    expect(relativeAgo(at(23, 17), now, 'en-US')).toBe('3h ago')
    expect(relativeAgo(at(15, 21, 52), now, 'en-US')).toBe('Oct 15')
  })
})
