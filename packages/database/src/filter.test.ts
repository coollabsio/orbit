import { describe, expect, test } from 'bun:test'
import { matches } from './filter'
import { ctx, database, property, row } from './testing'
import type { DatabaseFilterOperator, ViewContext } from './types'
import { parseDatabase } from './values'

function ids(propertyId: string, operator: DatabaseFilterOperator, value?: unknown, context: ViewContext = ctx) {
  const rule = { property_id: propertyId, operator, value }
  return database.rows.filter((row) => matches(row, rule, property(propertyId), context)).map((row) => row.id)
}

const all = ['r1', 'r2', 'r3', 'r4']

describe('text filters ignore case and treat a missing value as ""', () => {
  test.each([
    ['equals', 'CRUST', ['r2']],
    ['not_equals', 'crust', ['r1', 'r3', 'r4']],
    ['contains', 'crust', ['r1', 'r2']],
    ['not_contains', 'crust', ['r3', 'r4']],
    ['starts_with', 'sw', ['r1']],
    ['ends_with', 'UST', ['r1', 'r2']],
    ['contains', 'éclair', ['r4']],
    ['is_empty', undefined, ['r3']],
    ['is_not_empty', undefined, ['r1', 'r2', 'r4']],
  ] as const)('text %s %p', (operator, value, expected) => {
    expect(ids('p-text', operator, value)).toEqual([...expected])
  })

  test('a rule without a value matches every row', () => {
    expect(ids('p-text', 'contains', '')).toEqual(all)
    expect(ids('p-text', 'equals', null)).toEqual(all)
  })

  test('title and url use the same operators', () => {
    expect(ids('p-title', 'contains', 'AN')).toEqual(['r2'])
    expect(ids('p-title', 'is_empty')).toEqual(['r3'])
    expect(ids('p-url', 'contains', 'example')).toEqual(['r1', 'r2'])
    expect(ids('p-url', 'is_empty')).toEqual(['r3', 'r4'])
  })
})

describe('number filters', () => {
  test.each([
    ['=', 0, ['r4']],
    ['!=', 10, ['r2', 'r3', 'r4']],
    ['>', 0, ['r1']],
    ['<', 0, ['r2']],
    ['>=', 0, ['r1', 'r4']],
    ['<=', 0, ['r2', 'r4']],
    ['is_empty', undefined, ['r3']],
    ['is_not_empty', undefined, ['r1', 'r2', 'r4']],
    ['>', null, all],
  ] as const)('%s %p', (operator, value, expected) => {
    expect(ids('p-num', operator, value)).toEqual([...expected])
  })
})

describe('select and status filters match any listed option', () => {
  test('select', () => {
    expect(ids('p-sel', 'is', ['o-a', 'o-c'])).toEqual(['r1', 'r4'])
    expect(ids('p-sel', 'is_not', ['o-a'])).toEqual(['r2', 'r3', 'r4'])
    expect(ids('p-sel', 'is_empty')).toEqual(['r3'])
    expect(ids('p-sel', 'is_not_empty')).toEqual(['r1', 'r2', 'r4'])
    expect(ids('p-sel', 'is', [])).toEqual(all)
  })

  test('a status without a value is its first todo option, so it is never empty', () => {
    expect(ids('p-status', 'is', ['s-todo'])).toEqual(['r3', 'r4'])
    expect(ids('p-status', 'is_not', ['s-done'])).toEqual(['r2', 'r3', 'r4'])
    expect(ids('p-status', 'is_empty')).toEqual([])
    expect(ids('p-status', 'is_not_empty')).toEqual(all)
  })
})

describe('multi-select and person filters', () => {
  test('multi-select contains any listed option', () => {
    expect(ids('p-multi', 'contains', ['t-y', 't-z'])).toEqual(['r1', 'r2'])
    expect(ids('p-multi', 'not_contains', ['t-x'])).toEqual(['r2', 'r3', 'r4'])
    expect(ids('p-multi', 'is_empty')).toEqual(['r3', 'r4'])
    expect(ids('p-multi', 'is_not_empty')).toEqual(['r1', 'r2'])
  })

  test('person takes "me" alone or in the list', () => {
    expect(ids('p-person', 'contains', 'me')).toEqual(['r1'])
    expect(ids('p-person', 'contains', ['me', 'u-cy'])).toEqual(['r1', 'r2'])
    expect(ids('p-person', 'not_contains', ['u-bob'])).toEqual(['r1', 'r3', 'r4'])
    expect(ids('p-person', 'not_contains', 'me', { ...ctx, userId: 'u-bob' })).toEqual(['r1', 'r3', 'r4'])
    expect(ids('p-person', 'is_empty')).toEqual(['r3', 'r4'])
  })

  test('created by and last edited by', () => {
    expect(ids('p-cb', 'is', 'me')).toEqual(['r1', 'r4'])
    expect(ids('p-cb', 'is_not', ['me'])).toEqual(['r2', 'r3'])
    expect(ids('p-cb', 'is', ['u-bob', 'u-cy'])).toEqual(['r2', 'r3'])
    expect(ids('p-lb', 'is', 'me')).toEqual(['r2'])
    expect(ids('p-lb', 'is', [])).toEqual(all)
  })
})

test('checkbox is true or false', () => {
  expect(ids('p-check', 'is', true)).toEqual(['r1'])
  expect(ids('p-check', 'is', false)).toEqual(['r2', 'r3', 'r4'])
  expect(ids('p-check', 'is', null)).toEqual(all)
})

describe('date filters compare local days in the viewer zone (now is Nov 9, 05:00 in New York)', () => {
  test.each([
    ['is', 'today', ['r1', 'r2', 'r4']],
    ['is', 'tomorrow', ['r4']],
    ['is', '2026-11-10', ['r4']],
    ['is', '2026-11-10T02:00:00Z', ['r1', 'r2', 'r4']],
    ['before', 'today', ['r4']],
    ['after', 'yesterday', ['r1', 'r2']],
    ['on_or_before', '2026-11-02', ['r4']],
    ['on_or_after', 'one_week_ago', ['r1', 'r2', 'r4']],
    ['before', 'one_week_from_now', ['r1', 'r2', 'r4']],
    ['is_empty', undefined, ['r3']],
    ['is_not_empty', undefined, ['r1', 'r2', 'r4']],
    ['before', '', all],
  ] as const)('date %s %p', (operator, value, expected) => {
    expect(ids('p-date', operator, value)).toEqual([...expected])
  })

  test('a time falls on its day in the viewer zone', () => {
    expect(ids('p-date', 'is', '2026-11-10', { ...ctx, timeZone: 'UTC' })).toEqual(['r2', 'r4'])
  })

  test('created and last edited time', () => {
    expect(ids('p-ct', 'after', 'one_week_ago')).toEqual(['r3'])
    expect(ids('p-ct', 'on_or_after', 'one_week_ago')).toEqual(['r3', 'r4'])
    expect(ids('p-ct', 'is', 'one_month_ago')).toEqual(['r2'])
    expect(ids('p-ct', 'before', 'one_month_from_now')).toEqual(all)
    expect(ids('p-lt', 'is', 'yesterday')).toEqual(['r1', 'r3'])
    expect(ids('p-lt', 'is', 'today')).toEqual(['r2'])
    expect(ids('p-lt', 'is_empty')).toEqual([])
  })

  test('a month back from Mar 31 is Feb 28', () => {
    const march = parseDatabase({
      database: { id: 'db', title: 'Dates', icon: null, locked: false, row_count: 2 },
      properties: [{ id: 'p-date', name: 'Due', type: 'date', config: {}, position: 0 }],
      views: [],
      rows: [
        row('a', 0, 'Feb 28', { 'p-date': { start: '2026-02-28' } }),
        row('b', 1, 'Mar 3', { 'p-date': { start: '2026-03-03' } }),
      ],
    })
    const rule = { property_id: 'p-date', operator: 'is', value: 'one_month_ago' } as const
    const context = { ...ctx, now: Date.parse('2026-03-31T12:00:00Z') }
    const due = march.properties[0]!
    expect(march.rows.filter((row) => matches(row, rule, due, context)).map((row) => row.id)).toEqual(['a'])
  })
})
