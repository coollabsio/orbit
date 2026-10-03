import { expect, test } from 'bun:test'
import { dueRangeOf } from './dueRange'

const day = (date: number, hours = 0, minutes = 0) => new Date(2030, 0, date, hours, minutes)

test('one day is a due date without a start, also as a one-day range', () => {
  expect(dueRangeOf({ from: day(2), to: undefined }, '09:30')).toEqual({ start: null, end: day(2, 9, 30).toISOString() })
  expect(dueRangeOf({ from: day(2, 15), to: day(2) }, '09:30')).toEqual({ start: null, end: day(2, 9, 30).toISOString() })
})

test('a range starts at local midnight of its first day and ends on its last day at the time', () => {
  expect(dueRangeOf({ from: day(2, 15), to: day(5) }, '17:00')).toEqual({ start: day(2).toISOString(), end: day(5, 17).toISOString() })
  expect(dueRangeOf(undefined, '09:00')).toBeNull()
})
