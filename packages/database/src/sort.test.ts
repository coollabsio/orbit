import { describe, expect, test } from 'bun:test'
import { sortRows } from './sort'
import { ctx, database, properties, row } from './testing'
import type { DatabaseSortDirection } from './types'
import { parseDatabase } from './values'

function order(...sorts: [string, DatabaseSortDirection][]) {
  return sortRows(
    database.rows,
    sorts.map(([property_id, direction]) => ({ property_id, direction })),
    database.properties,
    ctx,
  ).map((row) => row.id)
}

describe('empty values go last in both directions', () => {
  test.each([
    ['p-text', ['r2', 'r4', 'r1', 'r3'], ['r1', 'r4', 'r2', 'r3']],
    ['p-title', ['r1', 'r2', 'r4', 'r3'], ['r4', 'r2', 'r1', 'r3']],
    ['p-num', ['r2', 'r4', 'r1', 'r3'], ['r1', 'r4', 'r2', 'r3']],
    ['p-date', ['r4', 'r1', 'r2', 'r3'], ['r2', 'r1', 'r4', 'r3']],
    ['p-url', ['r2', 'r1', 'r3', 'r4'], ['r1', 'r2', 'r3', 'r4']],
  ] as const)('%s', (propertyId, asc, desc) => {
    expect(order([propertyId, 'asc'])).toEqual([...asc])
    expect(order([propertyId, 'desc'])).toEqual([...desc])
  })
})

test('select and status sort by option order, not by name', () => {
  expect(order(['p-sel', 'asc'])).toEqual(['r1', 'r2', 'r4', 'r3'])
  expect(order(['p-sel', 'desc'])).toEqual(['r4', 'r2', 'r1', 'r3'])
  expect(order(['p-status', 'asc'])).toEqual(['r3', 'r4', 'r2', 'r1'])
  expect(order(['p-multi', 'desc'])).toEqual(['r2', 'r1', 'r3', 'r4'])
})

test('people sort by display name, not by id', () => {
  expect(order(['p-person', 'asc'])).toEqual(['r1', 'r2', 'r3', 'r4'])
  expect(order(['p-person', 'desc'])).toEqual(['r2', 'r1', 'r3', 'r4'])
  expect(order(['p-cb', 'asc'])).toEqual(['r1', 'r4', 'r2', 'r3'])
})

test('checkbox puts false before true', () => {
  expect(order(['p-check', 'asc'])).toEqual(['r2', 'r3', 'r4', 'r1'])
  expect(order(['p-check', 'desc'])).toEqual(['r1', 'r2', 'r3', 'r4'])
})

test('a later sort breaks ties of an earlier one', () => {
  expect(order(['p-status', 'asc'], ['p-num', 'desc'])).toEqual(['r4', 'r3', 'r2', 'r1'])
  expect(order(['p-check', 'asc'], ['p-title', 'desc'])).toEqual(['r4', 'r2', 'r3', 'r1'])
})

test('a sort on a deleted property is skipped', () => {
  expect(order(['gone', 'asc'], ['p-num', 'asc'])).toEqual(['r2', 'r4', 'r1', 'r3'])
})

test('ties fall back to position, then id', () => {
  const tied = parseDatabase({
    database: { id: 'db', title: 'Ties', icon: null, locked: false, row_count: 4 },
    properties,
    views: [],
    rows: [row('b', 1, 'Same', {}), row('c', 0, 'same', {}), row('a', 1, 'SAME', {}), row('d', 2, '', {})],
  })
  const sorted = sortRows(tied.rows, [{ property_id: 'p-title', direction: 'desc' }], tied.properties, ctx)
  expect(sorted.map((row) => row.id)).toEqual(['c', 'a', 'b', 'd'])
  expect(sortRows(tied.rows, [], tied.properties, ctx).map((row) => row.id)).toEqual(['c', 'a', 'b', 'd'])
})
