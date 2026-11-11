import { describe, expect, test } from 'bun:test'
import { calculate, calculationsFor } from './calculate'
import { matches } from './filter'
import { filterTypeOf } from './formula/values'
import { newRule, operatorsOf, ruleSummary, sortable } from './meta'
import {
  hiddenLinks,
  parseRelationConfig,
  parseRollupConfig,
  relatedTitle,
  rollupBase,
  rollupFunctionsFor,
  rollupHidesPages,
  withRelatedPages,
  type RollupFunction,
  type RollupProperty,
} from './relations'
import { sortRows } from './sort'
import { ctx as baseCtx, row } from './testing'
import type { CellValue, DatabaseFilterOperator, DatabaseProperty, Property, Row, ViewContext } from './types'
import { apiValue, displayText, parseDatabase, parseValue } from './values'

const database = parseDatabase({
  database: { id: 'db', title: 'Projects', icon: null, locked: false, row_count: 4 },
  properties: [
    { id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 },
    { id: 'p-rel', name: 'Tasks', type: 'relation', config: { database_id: 'db-tasks', limit: 'none', synced_property_id: 'p-back' }, position: 1 },
    { id: 'p-sum', name: 'Hours', type: 'rollup', config: { relation_property_id: 'p-rel', target_property_id: 't-hours', function: 'sum' }, position: 2 },
    { id: 'p-done', name: 'Done', type: 'rollup', config: { relation_property_id: 'p-rel', target_property_id: 't-check', function: 'percent_checked' }, position: 3 },
    { id: 'p-first', name: 'First due', type: 'rollup', config: { relation_property_id: 'p-rel', target_property_id: 't-due', function: 'earliest_date' }, position: 4 },
    { id: 'p-names', name: 'Owners', type: 'rollup', config: { relation_property_id: 'p-rel', target_property_id: 't-owner', function: 'show_original' }, position: 5 },
  ] satisfies DatabaseProperty[],
  views: [],
  rows: [
    row('r1', 0, 'Launch', { 'p-rel': ['pg-a', 'pg-b'], 'p-sum': 12, 'p-done': 0.5, 'p-first': { start: '2026-11-03', end: null }, 'p-names': ['Zed', 'Ann'] }),
    row('r2', 1, 'Docs', { 'p-rel': ['pg-c'], 'p-sum': 3.5, 'p-done': 1, 'p-first': { start: '2026-11-20', end: null }, 'p-names': ['Bea'] }),
    row('r3', 2, 'Empty', {}),
    row('r4', 3, 'Hidden', { 'p-rel': ['pg-gone', 'pg-untitled'], 'p-sum': 0, 'p-done': 0 }, { hidden_links: { 'p-rel': 2 } }),
  ],
})

const ctx: ViewContext = withRelatedPages(baseCtx, {
  related_pages: {
    'pg-a': { title: 'Write copy', icon: null, database_id: 'db-tasks' },
    'pg-b': { title: 'Ship site', icon: null, database_id: 'db-tasks' },
    'pg-c': { title: 'Draft API guide', icon: null, database_id: 'db-tasks' },
    'pg-untitled': { title: '', icon: null, database_id: 'db-tasks' },
  },
})

function property(id: string): Property {
  return database.properties.find((candidate) => candidate.id === id)!
}
const rollup = (id: string) => property(id) as RollupProperty

function ids(propertyId: string, operator: DatabaseFilterOperator, value?: unknown) {
  const rule = { property_id: propertyId, operator, value }
  return database.rows.filter((row) => matches(row, rule, property(propertyId), ctx)).map((row) => row.id)
}

function order(propertyId: string, direction: 'asc' | 'desc') {
  return sortRows(database.rows, [{ property_id: propertyId, direction }], database.properties, ctx).map((row) => row.id)
}

const cell = (id: string, rowId: string): CellValue | null => database.rows.find((row) => row.id === rowId)!.values[id] ?? null

describe('relation values', () => {
  test('a relation cell holds page ids; titles come from the related pages', () => {
    expect(cell('p-rel', 'r1')).toEqual({ kind: 'pages', ids: ['pg-a', 'pg-b'] })
    expect(displayText(property('p-rel'), cell('p-rel', 'r1'), ctx)).toBe('Write copy, Ship site')
    expect(displayText(property('p-rel'), cell('p-rel', 'r4'), ctx)).toBe('Untitled')
    expect(apiValue(cell('p-rel', 'r1'))).toEqual(['pg-a', 'pg-b'])
  })

  test('empty or repeated ids are no value', () => {
    expect(parseValue(property('p-rel'), [])).toBe(null)
    expect(parseValue(property('p-rel'), ['pg-a', 'pg-a'])).toBe(null)
    expect(parseValue(property('p-rel'), 'pg-a')).toBe(null)
  })

  test('relatedTitle: Untitled for an empty title, null for an unknown page', () => {
    expect(relatedTitle(ctx, 'pg-a')).toBe('Write copy')
    expect(relatedTitle(ctx, 'pg-untitled')).toBe('Untitled')
    expect(relatedTitle(ctx, 'pg-gone')).toBe(null)
    expect(relatedTitle(baseCtx, 'pg-a')).toBe(null)
  })

  test('withRelatedPages keeps the context when there are no pages', () => {
    expect(withRelatedPages(baseCtx, undefined)).toBe(baseCtx)
    expect(withRelatedPages(ctx, { related_pages: ctx.relatedPages })).toBe(ctx)
  })

  test('hidden links count per relation; a rollup over them hides pages', () => {
    const [r1, , , r4] = database.rows as [Row, Row, Row, Row]
    expect(hiddenLinks(r4, 'p-rel')).toBe(2)
    expect(hiddenLinks(r1, 'p-rel')).toBe(0)
    expect(rollupHidesPages(r4, rollup('p-sum'))).toBe(true)
    expect(rollupHidesPages(r1, rollup('p-sum'))).toBe(false)
  })
})

describe('relation filters match related titles', () => {
  test.each([
    ['contains', 'SHIP', ['r1']],
    ['contains', 'untitled', ['r4']],
    ['not_contains', 'guide', ['r1', 'r3', 'r4']],
    ['is_empty', undefined, ['r3']],
    ['is_not_empty', undefined, ['r1', 'r2', 'r4']],
    ['contains', '', ['r1', 'r2', 'r3', 'r4']],
  ] as const)('%s %p', (operator, value, expected) => {
    expect(ids('p-rel', operator, value)).toEqual([...expected])
  })

  test('a relation offers title operators and no sort', () => {
    expect(operatorsOf('relation')).toEqual(['contains', 'not_contains', 'is_empty', 'is_not_empty'])
    expect(sortable('relation')).toBe(false)
    expect(sortable('rollup')).toBe(true)
  })

  test('a relation sort keeps the manual order', () => {
    expect(order('p-rel', 'desc')).toEqual(['r1', 'r2', 'r3', 'r4'])
  })
})

describe('rollups filter, sort and calculate by their result', () => {
  test('a number rollup filters with number operators', () => {
    expect(filterTypeOf(property('p-sum'))).toBe('number')
    expect(newRule(property('p-sum'))).toEqual({ property_id: 'p-sum', operator: '=', value: null })
    expect(ids('p-sum', '>', 3)).toEqual(['r1', 'r2'])
    expect(ids('p-sum', '=', 0)).toEqual(['r4'])
    expect(ids('p-sum', 'is_empty')).toEqual(['r3'])
  })

  test('a percent rollup is a fraction', () => {
    expect(ids('p-done', '>=', 0.5)).toEqual(['r1', 'r2'])
    expect(displayText(property('p-done'), cell('p-done', 'r1'), ctx)).toBe('50%')
  })

  test('a date rollup filters with date operators', () => {
    expect(filterTypeOf(property('p-first'))).toBe('date')
    expect(ids('p-first', 'before', '2026-11-10')).toEqual(['r1'])
    expect(ids('p-first', 'on_or_after', '2026-11-03')).toEqual(['r1', 'r2'])
    expect(ruleSummary({ property_id: 'p-first', operator: 'before', value: '2026-11-10' }, property('p-first'), ctx)).toBe('is before 2026-11-10')
  })

  test('a list rollup filters on its display texts', () => {
    expect(filterTypeOf(property('p-names'))).toBe('rollup')
    expect(cell('p-names', 'r1')).toEqual({ kind: 'list', items: ['Zed', 'Ann'] })
    expect(ids('p-names', 'contains', 'ann')).toEqual(['r1'])
    expect(ids('p-names', 'not_contains', 'ann')).toEqual(['r2', 'r3', 'r4'])
    expect(ids('p-names', 'is_empty')).toEqual(['r3', 'r4'])
  })

  test.each([
    ['p-sum', ['r4', 'r2', 'r1', 'r3'], ['r1', 'r2', 'r4', 'r3']],
    ['p-first', ['r1', 'r2', 'r3', 'r4'], ['r2', 'r1', 'r3', 'r4']],
    ['p-names', ['r2', 'r1', 'r3', 'r4'], ['r1', 'r2', 'r3', 'r4']],
  ] as const)('%s sorts by result, empty last', (propertyId, asc, desc) => {
    expect(order(propertyId, 'asc')).toEqual([...asc])
    expect(order(propertyId, 'desc')).toEqual([...desc])
  })

  test('number functions take a number rollup', () => {
    expect(calculationsFor(filterTypeOf(property('p-sum')))).toContain('sum')
    expect(calculationsFor(filterTypeOf(property('p-names')))).not.toContain('sum')
    expect(calculate(database.rows, property('p-sum'), 'sum', ctx)).toEqual({ value: '15.5', raw: 15.5 })
    expect(calculate(database.rows, property('p-done'), 'average', ctx)).toEqual({ value: '50%', raw: 0.5 })
    expect(calculate(database.rows, property('p-names'), 'count_values', ctx)).toEqual({ value: '3', raw: 3 })
    expect(calculate(database.rows, property('p-rel'), 'count_unique', ctx)).toEqual({ value: '5', raw: 5 })
  })

  test('rollupBase reads number, percent, date and list results as stored types', () => {
    expect(rollupBase(rollup('p-sum'))).toEqual({ id: 'p-sum', name: 'Hours', type: 'number', config: { format: 'number' }, position: 2 })
    expect(rollupBase(rollup('p-done')).config).toEqual({ format: 'percent' })
    expect(rollupBase(rollup('p-first')).type).toBe('date')
    expect(rollupBase(rollup('p-names')).type).toBe('text')
  })
})

describe('rollup configs', () => {
  const target = (type: DatabaseProperty['type'], config: Record<string, unknown> = {}) =>
    parseDatabase({ database: { id: 't', title: '', icon: null, locked: false, row_count: 0 }, properties: [{ id: 't', name: 'T', type, config, position: 0 }], views: [], rows: [] }).properties[0]!

  test('functions fit the target type', () => {
    const counts: RollupFunction[] = ['show_original', 'show_unique', 'count_all', 'count_values', 'count_unique', 'count_empty', 'count_not_empty', 'percent_empty', 'percent_not_empty']
    expect(rollupFunctionsFor(target('text'))).toEqual(counts)
    expect(rollupFunctionsFor(target('checkbox'))).toEqual([...counts, 'checked', 'unchecked', 'percent_checked', 'percent_unchecked'])
    expect(rollupFunctionsFor(target('unique_id'))).toEqual([...counts, 'sum', 'average', 'median', 'min', 'max', 'range'])
    expect(rollupFunctionsFor(target('created_time'))).toEqual([...counts, 'earliest_date', 'latest_date', 'date_range'])
    expect(rollupFunctionsFor(rollup('p-first'))).toEqual([...counts, 'earliest_date', 'latest_date', 'date_range'])
    expect(rollupFunctionsFor(rollup('p-names'))).toEqual(counts)
  })

  test('malformed configs get defaults', () => {
    expect(parseRelationConfig({})).toEqual({ database_id: '', limit: 'none', synced_property_id: null, primary: true })
    expect(parseRelationConfig({ database_id: 'd', limit: 'one', synced_property_id: 'p', primary: false })).toEqual({ database_id: 'd', limit: 'one', synced_property_id: 'p', primary: false })
    expect(parseRollupConfig({ relation_property_id: 'r', target_property_id: 7, function: 'nope' })).toEqual({ relation_property_id: 'r', target_property_id: '', function: 'show_original' })
  })
})
