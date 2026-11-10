import { describe, expect, test } from 'bun:test'
import { calculate, calculationsFor } from './calculate'
import { ctx, database, property, row } from './testing'
import type { DatabaseCalculation, Row } from './types'
import { parseRow, parseProperty } from './values'

const rows = database.rows
const one = rows.slice(0, 1)
const value = (fn: DatabaseCalculation, propertyId: string, over: readonly Row[] = rows) => calculate(over, property(propertyId), fn, ctx).value

describe('counts', () => {
  test.each([
    ['count_all', 'p-text', '0', '1', '4'],
    ['count_values', 'p-text', '0', '1', '3'],
    ['count_empty', 'p-text', '0', '0', '1'],
    ['count_not_empty', 'p-text', '0', '1', '3'],
    ['percent_empty', 'p-text', '0%', '0%', '25%'],
    ['percent_not_empty', 'p-text', '0%', '100%', '75%'],
    ['count_unique', 'p-sel', '0', '1', '3'],
  ] as const)('%s on empty, one and many rows', (fn, propertyId, empty, single, many) => {
    expect([value(fn, propertyId, []), value(fn, propertyId, one), value(fn, propertyId)]).toEqual([empty, single, many])
  })

  test('a multi-select or person cell counts each item; unique counts distinct items', () => {
    const extra = parseRow(row('r5', 4, 'Again', { 'p-multi': ['t-x'] }), database.properties)
    expect(value('count_values', 'p-multi', [...rows, extra])).toBe('4')
    expect(value('count_unique', 'p-multi', [...rows, extra])).toBe('3')
    expect(value('count_values', 'p-person')).toBe('3')
  })

  test('a status without a value counts as its default option', () => {
    expect(value('count_empty', 'p-status')).toBe('0')
    expect(value('count_unique', 'p-status')).toBe('3')
  })

  test('percentages round to one decimal', () => {
    expect(value('percent_empty', 'p-num', rows.slice(1))).toBe('33.3%')
    expect(calculate(rows.slice(1), property('p-num'), 'percent_not_empty', ctx).raw).toBe(66.7)
  })

  test('checkbox functions', () => {
    expect([value('checked', 'p-check'), value('unchecked', 'p-check'), value('percent_checked', 'p-check'), value('percent_unchecked', 'p-check')]).toEqual(['1', '3', '25%', '75%'])
  })
})

describe('numbers', () => {
  test.each([
    ['sum', '0', '10', '7.5'],
    ['average', '—', '10', '2.5'],
    ['median', '—', '10', '0'],
    ['min', '—', '10', '-2.5'],
    ['max', '—', '10', '10'],
    ['range', '—', '0', '12.5'],
  ] as const)('%s on empty, one and many rows', (fn, empty, single, many) => {
    expect([value(fn, 'p-num', []), value(fn, 'p-num', one), value(fn, 'p-num')]).toEqual([empty, single, many])
  })

  test('the median of an even count is the mean of the middle two', () => {
    expect(value('median', 'p-num', rows.slice(0, 2))).toBe('3.75')
  })

  test('results show in the property format, an average with at most 4 decimals', () => {
    const percent = parseProperty({ id: 'p-num', name: 'Share', type: 'number', config: { format: 'percent' }, position: 2 })
    expect(calculate(rows, percent, 'sum', ctx).value).toBe('750%')
    const thirds = [10, 0, 0].map((n, index) => parseRow(row(`n${index}`, index, '', { 'p-num': n }), database.properties))
    expect(value('average', 'p-num', thirds)).toBe('3.3333')
  })
})

describe('dates', () => {
  test('earliest and latest use the range ends, in the property format', () => {
    expect(value('earliest_date', 'p-date')).toBe('Nov 2, 2026')
    expect(value('latest_date', 'p-date')).toBe('Nov 12, 2026')
    expect(value('earliest_date', 'p-date', [])).toBe('—')
  })

  test('a date range reads in the largest whole unit', () => {
    expect(value('date_range', 'p-date')).toBe('10 days')
    expect(value('date_range', 'p-date', rows.slice(0, 2))).toBe('22 hours')
    expect(value('date_range', 'p-date', one)).toBe('0 days')
  })
})

test('each type offers its functions; checkbox has no empty counts', () => {
  expect(calculationsFor('checkbox')).toEqual(['count_all', 'checked', 'unchecked', 'percent_checked', 'percent_unchecked'])
  expect(calculationsFor('number')).toContain('median')
  expect(calculationsFor('created_time')).toContain('date_range')
  expect(calculationsFor('person')).not.toContain('sum')
})
