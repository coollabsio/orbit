import { describe, expect, test } from 'bun:test'
import {
  barsOn,
  calendarWeeks,
  datable,
  dayDate,
  dayDiff,
  defaultDateProperty,
  localeWeekStart,
  monthTitle,
  moveDate,
  placeRows,
  rowSpan,
  startTime,
  undatedRows,
  weekdayName,
  weekOf,
  withEndDay,
} from './calendar'
import { ctx, database, property, row } from './testing'
import type { DateProperty, DateValue, Row } from './types'
import { parseRow } from './values'

const due = property('p-date') as DateProperty
const dated = (id: string, position: number, date: unknown): Row => parseRow(row(id, position, id, { 'p-date': date }), database.properties)
const day = (value: string) => ({ kind: 'day' as const, day: value })
const moment = (iso: string) => ({ kind: 'moment' as const, iso, ms: Date.parse(iso) })

describe('calendarWeeks', () => {
  test('a month that ends a year runs into January, Monday first', () => {
    const weeks = calendarWeeks('2026-12-15', 1)
    expect(weeks.map((week) => [week[0], week[6]])).toEqual([
      ['2026-11-30', '2026-12-06'],
      ['2026-12-07', '2026-12-13'],
      ['2026-12-14', '2026-12-20'],
      ['2026-12-21', '2026-12-27'],
      ['2026-12-28', '2027-01-03'],
      ['2027-01-04', '2027-01-10'],
    ])
    expect(weeks[4]).toEqual(['2026-12-28', '2026-12-29', '2026-12-30', '2026-12-31', '2027-01-01', '2027-01-02', '2027-01-03'])
  })

  test('a January starts in the old year, Sunday first', () => {
    expect(calendarWeeks('2027-01-20', 0)[0]).toEqual(['2026-12-27', '2026-12-28', '2026-12-29', '2026-12-30', '2026-12-31', '2027-01-01', '2027-01-02'])
  })

  test('the weeks of a DST change hold 7 plain days each', () => {
    expect(calendarWeeks('2026-03-01', 0)[1]).toEqual(['2026-03-08', '2026-03-09', '2026-03-10', '2026-03-11', '2026-03-12', '2026-03-13', '2026-03-14'])
    expect(calendarWeeks('2026-11-30', 6)[0]).toEqual(['2026-10-31', '2026-11-01', '2026-11-02', '2026-11-03', '2026-11-04', '2026-11-05', '2026-11-06'])
  })

  test('weekOf starts on the week start before the day', () => {
    expect(weekOf('2026-11-09', 1)[0]).toBe('2026-11-09')
    expect(weekOf('2026-11-09', 0)[0]).toBe('2026-11-08')
    expect(weekOf('2026-11-09', 6)[0]).toBe('2026-11-07')
  })
})

describe('rowSpan', () => {
  test("a moment counts as its day in the viewer's zone", () => {
    const late = dated('a', 0, { start: '2026-11-10T03:00:00Z', end: null })
    expect(rowSpan(late, due, 'America/New_York')).toEqual({ start: '2026-11-09', end: '2026-11-09' })
    expect(rowSpan(late, due, 'UTC')).toEqual({ start: '2026-11-10', end: '2026-11-10' })
  })

  test('a range covers both ends; a row without a date has no span', () => {
    expect(rowSpan(database.rows[3]!, due, ctx.timeZone)).toEqual({ start: '2026-11-02', end: '2026-11-12' })
    expect(rowSpan(database.rows[2]!, due, ctx.timeZone)).toBe(null)
    expect(undatedRows(database.rows, due, ctx.timeZone).map((row) => row.id)).toEqual(['r3'])
  })
})

describe('placeRows', () => {
  const range = dated('range', 0, { start: '2026-11-05', end: '2026-11-10' })
  const inside = dated('inside', 1, { start: '2026-11-06', end: null })
  const early = dated('early', 2, { start: '2026-11-02', end: null })
  const rows = [range, inside, early, dated('later', 3, { start: '2026-11-20', end: null })]

  test('a range across two weeks continues from one week into the next', () => {
    const [first, second] = calendarWeeks('2026-11-01', 0)
    const bars = (week: string[]) =>
      placeRows(rows, due, week, ctx.timeZone).map(({ row, column, length, lane, continuesBefore, continuesAfter }) => ({
        id: row.id,
        column,
        length,
        lane,
        continuesBefore,
        continuesAfter,
      }))
    expect(bars(first!)).toEqual([
      { id: 'early', column: 1, length: 1, lane: 0, continuesBefore: false, continuesAfter: false },
      { id: 'range', column: 4, length: 3, lane: 0, continuesBefore: false, continuesAfter: true },
      { id: 'inside', column: 5, length: 1, lane: 1, continuesBefore: false, continuesAfter: false },
    ])
    expect(bars(second!)).toEqual([{ id: 'range', column: 0, length: 3, lane: 0, continuesBefore: true, continuesAfter: false }])
  })

  test('barsOn gives the bars of a day in lane order', () => {
    const bars = placeRows(rows, due, calendarWeeks('2026-11-01', 0)[0]!, ctx.timeZone)
    expect(barsOn(bars, 5).map((bar) => bar.row.id)).toEqual(['range', 'inside'])
    expect(barsOn(bars, 0)).toEqual([])
    expect(barsOn(bars, 1).map((bar) => bar.row.id)).toEqual(['early'])
  })

  test('the same start puts the longer range first', () => {
    const short = dated('short', 0, { start: '2026-11-03', end: null })
    const long = dated('long', 1, { start: '2026-11-03', end: '2026-11-05' })
    const bars = placeRows([short, long], due, calendarWeeks('2026-11-01', 0)[0]!, ctx.timeZone)
    expect(bars.map((bar) => [bar.row.id, bar.lane])).toEqual([
      ['long', 0],
      ['short', 1],
    ])
  })
})

describe('moveDate', () => {
  test('a day range keeps its length', () => {
    const value: DateValue = { start: day('2026-11-05'), end: day('2026-11-07') }
    expect(moveDate(value, -7, 'UTC')).toEqual({ start: day('2026-10-29'), end: day('2026-10-31') })
  })

  test('a moment keeps its time', () => {
    expect(moveDate({ start: moment('2026-11-09T14:30:00Z'), end: null }, 2, 'UTC')).toEqual({ start: moment('2026-11-11T14:30:00Z'), end: null })
  })

  test('across a DST change a moment keeps its local hour in the zone', () => {
    const value: DateValue = { start: moment('2026-10-31T13:00:00Z'), end: moment('2026-10-31T15:00:00Z') }
    expect(moveDate(value, 1, 'America/New_York')).toEqual({ start: moment('2026-11-01T14:00:00Z'), end: moment('2026-11-01T16:00:00Z') })
    expect(moveDate({ start: moment('2026-10-30T13:30:00Z'), end: null }, 3, 'America/New_York')).toEqual({ start: moment('2026-11-02T14:30:00Z'), end: null })
    expect(moveDate(value, 1, 'UTC')).toEqual({ start: moment('2026-11-01T13:00:00Z'), end: moment('2026-11-01T15:00:00Z') })
  })
})

describe('withEndDay', () => {
  test('a day value gets an end, none on its start day, and no end before its start', () => {
    const value: DateValue = { start: day('2026-11-05'), end: null }
    expect(withEndDay(value, '2026-11-08', 'UTC')).toEqual({ start: day('2026-11-05'), end: day('2026-11-08') })
    expect(withEndDay({ start: day('2026-11-05'), end: day('2026-11-08') }, '2026-11-05', 'UTC')).toEqual({ start: day('2026-11-05'), end: null })
    expect(withEndDay(value, '2026-11-04', 'UTC')).toBe(null)
  })

  test("a moment's end keeps the end's local hour", () => {
    const value: DateValue = { start: moment('2026-11-09T14:00:00Z'), end: moment('2026-11-09T15:00:00Z') }
    expect(withEndDay(value, '2026-11-11', 'UTC')).toEqual({ start: moment('2026-11-09T14:00:00Z'), end: moment('2026-11-11T15:00:00Z') })
    expect(withEndDay(value, '2026-11-08', 'UTC')).toBe(null)
  })

  test("without an end, the start's local hour on the day in the zone", () => {
    const value: DateValue = { start: moment('2026-11-09T03:00:00Z'), end: null }
    expect(withEndDay(value, '2026-11-09', 'America/New_York')).toEqual({ start: moment('2026-11-09T03:00:00Z'), end: moment('2026-11-10T03:00:00Z') })
    expect(withEndDay(value, '2026-11-08', 'America/New_York')).toEqual({ start: moment('2026-11-09T03:00:00Z'), end: null })
    expect(withEndDay(value, '2026-11-07', 'America/New_York')).toBe(null)
  })
})

describe('calendar helpers', () => {
  test('datable and the default date property', () => {
    expect(['p-date', 'p-ct', 'p-lt', 'p-text'].map((id) => datable(property(id)))).toEqual([true, true, true, false])
    expect(datable(undefined)).toBe(false)
    expect(defaultDateProperty(database.properties)).toBe('p-date')
    expect(defaultDateProperty([property('p-text'), property('p-ct')])).toBe(null)
  })

  test('localeWeekStart reads the locale, Monday when unknown', () => {
    expect(localeWeekStart('en-US')).toBe(0)
    expect(localeWeekStart('en-GB')).toBe(1)
    expect(localeWeekStart('not a locale!')).toBe(1)
  })

  test('startTime shows a moment in the zone by the time format; a day has none', () => {
    const late = dated('a', 0, { start: '2026-11-10T03:00:00Z', end: null })
    expect(startTime(late, due, ctx)).toBe('22:00')
    expect(startTime(late, { ...due, config: { date_format: 'short', time_format: '12h' } } as DateProperty, ctx)).toBe('10:00 PM')
    expect(startTime(database.rows[0]!, due, ctx)).toBe(null)
  })

  test('dayDate, dayDiff, monthTitle and weekdayName', () => {
    expect(dayDate('2026-11-09')).toEqual({ start: day('2026-11-09'), end: null })
    expect(dayDiff('2026-12-30', '2027-01-02')).toBe(3)
    expect(dayDiff('2026-11-09', '2026-11-02')).toBe(-7)
    expect(monthTitle('2026-11-09', 'en-US')).toBe('November 2026')
    expect(weekdayName('2026-11-09', 'en-US')).toBe('Mon')
    expect(weekdayName('2026-11-09', 'en-US', 'long')).toBe('Monday')
  })
})
