import { describe, expect, test } from 'bun:test'
import { axis, barOf, dayIndex, daysBetween, moveBar, spanValues, timelineFields, timelineOf, timelineRange, type Bar, type Timeline, type TimelineZoom } from './timeline'
import { row } from './testing'
import type { DatabaseProperty, Property, Row } from './types'
import { parseProperty, parseRow } from './values'

const properties: Property[] = (
  [
    { id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 },
    { id: 'p-text', name: 'Notes', type: 'text', config: {}, position: 1 },
    { id: 'p-start', name: 'Start', type: 'date', config: {}, position: 2 },
    { id: 'p-end', name: 'End', type: 'date', config: {}, position: 3 },
    { id: 'p-ct', name: 'Created', type: 'created_time', config: {}, position: 4 },
  ] satisfies DatabaseProperty[]
).map(parseProperty)

const NEW_YORK = 'America/New_York'
const BUDAPEST = 'Europe/Budapest'

const parsed = (values: Record<string, unknown>): Row => parseRow(row('r1', 0, 'Launch', values), properties)
const timeline = (config: Parameters<typeof timelineFields>[1] = {}): Timeline => timelineOf(timelineFields('timeline', config, properties) as Parameters<typeof timelineOf>[0], properties)
const ranged = timeline({ start_property_id: 'p-start' })
const twoDates = timeline({ start_property_id: 'p-start', end_property_id: 'p-end' })
const ids = (t: Timeline) => ({ start: t.start?.id ?? null, end: t.end?.id ?? null, zoom: t.zoom, showTable: t.showTable, columns: t.columns.map((p) => p.id) })

describe('timelineFields and timelineOf', () => {
  test('an empty config starts at the first date property with the defaults', () => {
    expect(timelineFields('timeline', {}, properties)).toEqual({ start_property_id: 'p-start' })
    expect(ids(timeline())).toEqual({ start: 'p-start', end: null, zoom: 'month', showTable: true, columns: [] })
  })

  test('a deleted end property is dropped and the bar ends at the start property', () => {
    expect(timelineFields('timeline', { start_property_id: 'p-start', end_property_id: 'p-gone' }, properties)).toEqual({ start_property_id: 'p-start' })
  })

  test('an end equal to the start is dropped', () => {
    expect(timelineFields('timeline', { start_property_id: 'p-end', end_property_id: 'p-end' }, properties)).toEqual({ start_property_id: 'p-end' })
  })

  test('a start that is not a date falls back to the first date property; an end that is not one is dropped', () => {
    expect(timelineFields('timeline', { start_property_id: 'p-ct', end_property_id: 'p-text' }, properties)).toEqual({ start_property_id: 'p-start' })
  })

  test('non-default zoom and show_table are kept; table columns lose the title, unknown ids and repeats', () => {
    const config = { start_property_id: 'p-start', end_property_id: 'p-end', zoom: 'week' as const, show_table: false, table_columns: ['p-text', 'p-title', 'p-gone', 'p-text', 'p-end'] }
    expect(timelineFields('timeline', config, properties)).toEqual({
      start_property_id: 'p-start',
      end_property_id: 'p-end',
      zoom: 'week',
      show_table: false,
      table_columns: ['p-text', 'p-end'],
    })
    expect(ids(timeline(config))).toEqual({ start: 'p-start', end: 'p-end', zoom: 'week', showTable: false, columns: ['p-text', 'p-end'] })
  })

  test('a view that is not a timeline gets no start property by default', () => {
    expect(timelineFields('table', {}, properties)).toEqual({})
  })
})

describe('barOf', () => {
  test('without an end property a date range is the bar', () => {
    expect(barOf(parsed({ 'p-start': { start: '2026-11-02', end: '2026-11-05' } }), ranged, NEW_YORK)).toEqual({
      span: { start: { kind: 'day', day: '2026-11-02' }, end: { kind: 'day', day: '2026-11-05' } },
      startDay: '2026-11-02',
      endDay: '2026-11-05',
    })
  })

  test('with an end property the bar ends at its start date; without its value the bar is one day', () => {
    const values = { 'p-start': { start: '2026-11-02', end: '2026-11-03' }, 'p-end': { start: '2026-11-20', end: null } }
    expect(barOf(parsed(values), twoDates, NEW_YORK)).toEqual({
      span: { start: { kind: 'day', day: '2026-11-02' }, end: { kind: 'day', day: '2026-11-20' } },
      startDay: '2026-11-02',
      endDay: '2026-11-20',
    })
    expect(barOf(parsed({ 'p-start': { start: '2026-11-02', end: null } }), twoDates, NEW_YORK)).toEqual({
      span: { start: { kind: 'day', day: '2026-11-02' }, end: { kind: 'day', day: '2026-11-02' } },
      startDay: '2026-11-02',
      endDay: '2026-11-02',
    })
  })

  test('an end before the start is ignored', () => {
    const values = { 'p-start': { start: '2026-11-10', end: null }, 'p-end': { start: '2026-11-01', end: null } }
    expect(barOf(parsed(values), twoDates, NEW_YORK)).toEqual({
      span: { start: { kind: 'day', day: '2026-11-10' }, end: { kind: 'day', day: '2026-11-10' } },
      startDay: '2026-11-10',
      endDay: '2026-11-10',
    })
  })

  test('a row without a start date has no bar', () => {
    expect([barOf(parsed({ 'p-end': { start: '2026-11-01', end: null } }), twoDates, NEW_YORK), barOf(parsed({ 'p-start': { start: '2026-11-01', end: null } }), twoDates, NEW_YORK)?.startDay]).toEqual([null, '2026-11-01'])
  })

  test('a moment range across the end of DST places by local days in the viewer zone', () => {
    // 23:30 EDT on Oct 30 to 23:30 EST on Nov 1 (New York leaves DST on Nov 1).
    const value = { 'p-start': { start: '2026-10-31T03:30:00Z', end: '2026-11-02T04:30:00Z' } }
    expect(barOf(parsed(value), ranged, NEW_YORK)).toEqual({
      span: {
        start: { kind: 'moment', iso: '2026-10-31T03:30:00Z', ms: Date.parse('2026-10-31T03:30:00Z') },
        end: { kind: 'moment', iso: '2026-11-02T04:30:00Z', ms: Date.parse('2026-11-02T04:30:00Z') },
      },
      startDay: '2026-10-30',
      endDay: '2026-11-01',
    })
    expect([barOf(parsed(value), ranged, 'UTC')?.startDay, barOf(parsed(value), ranged, 'UTC')?.endDay]).toEqual(['2026-10-31', '2026-11-02'])
  })
})

describe('bar placement on the axis', () => {
  const bar = barOf(parsed({ 'p-start': { start: '2026-11-02', end: '2026-11-05' } }), ranged, NEW_YORK)!
  const placement = (zoom: TimelineZoom) => {
    const a = axis(zoom, timelineRange([bar], '2026-11-10', zoom), 'en-US')
    return { start: a.start, days: a.days, left: dayIndex(a.start, bar.startDay) * a.dayWidth, width: (daysBetween(bar.startDay, bar.endDay) + 1) * a.dayWidth }
  }

  test('each zoom places the bar by whole local days', () => {
    expect(placement('day')).toEqual({ start: '2026-10-01', days: 61, left: 32 * 96, width: 4 * 96 })
    expect(placement('week')).toEqual({ start: '2026-09-01', days: 122, left: 62 * 36, width: 4 * 36 })
    expect(placement('month')).toEqual({ start: '2026-07-01', days: 274, left: 124 * 12, width: 4 * 12 })
    expect(placement('quarter')).toEqual({ start: '2025-11-01', days: 760, left: 366 * 4, width: 4 * 4 })
    expect(placement('year')).toEqual({ start: '2024-01-01', days: 1827, left: 1036 * 1.5, width: 4 * 1.5 })
  })

  test('the month axis labels months on top and Monday weeks below', () => {
    const a = axis('month', { start: '2026-11-01', end: '2026-11-30' }, 'en-US')
    expect(a.groups).toEqual([{ start: 0, days: 30, label: 'November 2026', day: '2026-11-01' }])
    expect(a.ticks.map((tick) => [tick.start, tick.days, tick.label])).toEqual([
      [0, 1, 'Nov 1'],
      [1, 7, 'Nov 2'],
      [8, 7, 'Nov 9'],
      [15, 7, 'Nov 16'],
      [22, 7, 'Nov 23'],
      [29, 1, 'Nov 30'],
    ])
  })

  test('a range across a DST change has one axis day per calendar day', () => {
    const a = axis('day', { start: '2026-03-01', end: '2026-03-31' }, 'en-US')
    expect([a.days, a.ticks.length, a.ticks[28], dayIndex(a.start, '2026-03-30')]).toEqual([31, 31, { start: 28, days: 1, label: '29 Sun', day: '2026-03-29' }, 29])
    expect(axis('quarter', { start: '2026-01-01', end: '2026-06-30' }, 'en-US').groups).toEqual([
      { start: 0, days: 90, label: 'Q1 2026', day: '2026-01-01' },
      { start: 90, days: 91, label: 'Q2 2026', day: '2026-04-01' },
    ])
  })
})

const day = (d: string) => ({ kind: 'day' as const, day: d })
const moment = (iso: string) => ({ kind: 'moment' as const, iso, ms: Date.parse(iso) })
const dayBar: Bar = { span: { start: day('2026-11-02'), end: day('2026-11-05') }, startDay: '2026-11-02', endDay: '2026-11-05' }

describe('moveBar', () => {
  test("'both' moves start and end by the days", () => {
    expect(moveBar(dayBar, 3, 'both', NEW_YORK)).toEqual({ start: day('2026-11-05'), end: day('2026-11-08') })
    expect(moveBar(dayBar, -5, 'both', NEW_YORK)).toEqual({ start: day('2026-10-28'), end: day('2026-10-31') })
  })

  test("'start' moves only the start and stops at the end", () => {
    expect(moveBar(dayBar, -2, 'start', NEW_YORK)).toEqual({ start: day('2026-10-31'), end: day('2026-11-05') })
    expect(moveBar(dayBar, 10, 'start', NEW_YORK)).toEqual({ start: day('2026-11-05'), end: day('2026-11-05') })
  })

  test("'end' moves only the end and stops at the start", () => {
    expect(moveBar(dayBar, 4, 'end', NEW_YORK)).toEqual({ start: day('2026-11-02'), end: day('2026-11-09') })
    expect(moveBar(dayBar, -10, 'end', NEW_YORK)).toEqual({ start: day('2026-11-02'), end: day('2026-11-02') })
  })

  test('a moment keeps its local hour when moved across the end of DST', () => {
    // 09:00 EDT (UTC-4) on Oct 30 to 17:00 EDT; three days later it is 09:00 and 17:00 EST (UTC-5).
    const bar: Bar = { span: { start: moment('2026-10-30T13:00:00Z'), end: moment('2026-10-30T21:00:00Z') }, startDay: '2026-10-30', endDay: '2026-10-30' }
    expect(moveBar(bar, 3, 'both', NEW_YORK)).toEqual({ start: moment('2026-11-02T14:00:00Z'), end: moment('2026-11-02T22:00:00Z') })
  })

  test('a moment keeps its local hour when its end moves across the start of DST', () => {
    // 10:00 CET (UTC+1) on Mar 27; two days later Budapest is on CEST (UTC+2).
    const bar: Bar = { span: { start: moment('2026-03-27T09:00:00Z'), end: moment('2026-03-27T09:00:00Z') }, startDay: '2026-03-27', endDay: '2026-03-27' }
    expect(moveBar(bar, 2, 'end', BUDAPEST)).toEqual({ start: moment('2026-03-27T09:00:00Z'), end: moment('2026-03-29T08:00:00Z') })
  })
})

describe('spanValues', () => {
  test('with an end property only the changed dates are written', () => {
    const r = parsed({ 'p-start': { start: '2026-11-02', end: null }, 'p-end': { start: '2026-11-05', end: null } })
    expect(spanValues(r, twoDates, { start: day('2026-11-02'), end: day('2026-11-09') }, NEW_YORK)).toEqual({ 'p-end': { start: '2026-11-09', end: null } })
    expect(spanValues(r, twoDates, { start: day('2026-10-30'), end: day('2026-11-05') }, NEW_YORK)).toEqual({ 'p-start': { start: '2026-10-30', end: null } })
    expect(spanValues(r, twoDates, { start: day('2026-11-03'), end: day('2026-11-06') }, NEW_YORK)).toEqual({
      'p-start': { start: '2026-11-03', end: null },
      'p-end': { start: '2026-11-06', end: null },
    })
  })

  test('with an empty end property a one-day bar writes no end, a longer one does', () => {
    const r = parsed({ 'p-start': { start: '2026-11-02', end: null } })
    expect(spanValues(r, twoDates, { start: day('2026-11-04'), end: day('2026-11-04') }, NEW_YORK)).toEqual({ 'p-start': { start: '2026-11-04', end: null } })
    expect(spanValues(r, twoDates, { start: day('2026-11-02'), end: day('2026-11-03') }, NEW_YORK)).toEqual({ 'p-end': { start: '2026-11-03', end: null } })
  })

  test('without an end property the start property holds the range; a one-day single date stays single', () => {
    const single = parsed({ 'p-start': { start: '2026-11-02', end: null } })
    expect(spanValues(single, ranged, { start: day('2026-11-04'), end: day('2026-11-04') }, NEW_YORK)).toEqual({ 'p-start': { start: '2026-11-04', end: null } })
    expect(spanValues(single, ranged, { start: day('2026-11-02'), end: day('2026-11-06') }, NEW_YORK)).toEqual({ 'p-start': { start: '2026-11-02', end: '2026-11-06' } })
    const range = parsed({ 'p-start': { start: '2026-11-02', end: '2026-11-02' } })
    expect(spanValues(range, ranged, { start: day('2026-11-03'), end: day('2026-11-03') }, NEW_YORK)).toEqual({ 'p-start': { start: '2026-11-03', end: '2026-11-03' } })
  })
})
