import { describe, expect, test } from 'bun:test'
import { newRowValues } from './newRow'
import { ctx, database, property } from './testing'
import type { DatabaseFilterRule } from './types'
import { numberEditText, numberFromEditText, parseProperty } from './values'

function fill(...rules: DatabaseFilterRule[]) {
  return newRowValues({ op: 'and', children: rules }, database.properties, ctx)
}

describe('a new row under a filter gets the values the filter needs', () => {
  test('select, status, multi-select, person and checkbox take the first listed id', () => {
    expect(
      fill(
        { property_id: 'p-status', operator: 'is', value: ['s-prog', 's-done'] },
        { property_id: 'p-sel', operator: 'is', value: ['o-b'] },
        { property_id: 'p-multi', operator: 'contains', value: ['t-z'] },
        { property_id: 'p-person', operator: 'contains', value: ['me'] },
        { property_id: 'p-check', operator: 'is', value: true },
      ),
    ).toEqual({
      title: '',
      values: { 'p-status': 's-prog', 'p-sel': 'o-b', 'p-multi': ['t-z'], 'p-person': ['u-me'], 'p-check': true },
    })
  })

  test('text, title, url and numbers', () => {
    expect(
      fill(
        { property_id: 'p-title', operator: 'starts_with', value: 'Paper' },
        { property_id: 'p-text', operator: 'contains', value: 'crust' },
        { property_id: 'p-url', operator: 'equals', value: 'https://example.com' },
        { property_id: 'p-num', operator: '>', value: 4 },
      ),
    ).toEqual({
      title: 'Paper',
      values: { 'p-text': 'crust', 'p-url': 'https://example.com', 'p-num': 5 },
    })
  })

  test('dates resolve relative values in the viewer zone and step past before/after', () => {
    expect(fill({ property_id: 'p-date', operator: 'is', value: 'tomorrow' })).toEqual({
      title: '',
      values: { 'p-date': { start: '2026-11-10', end: null } },
    })
    expect(fill({ property_id: 'p-date', operator: 'before', value: '2026-11-09' })).toEqual({
      title: '',
      values: { 'p-date': { start: '2026-11-08', end: null } },
    })
  })

  test('rules without one fitting value add nothing', () => {
    expect(
      fill(
        { property_id: 'p-status', operator: 'is_not', value: ['s-done'] },
        { property_id: 'p-text', operator: 'is_not_empty' },
        { property_id: 'p-text', operator: 'contains', value: '' },
        { property_id: 'p-sel', operator: 'is', value: ['deleted-option'] },
        { property_id: 'p-cb', operator: 'is', value: ['me'] },
        { property_id: 'p-check', operator: 'is', value: false },
      ),
    ).toEqual({ title: '', values: {} })
  })

  test('a value that breaks another rule on the same property is left out', () => {
    expect(
      fill(
        { property_id: 'p-num', operator: '=', value: 3 },
        { property_id: 'p-num', operator: '>', value: 10 },
      ),
    ).toEqual({ title: '', values: {} })
    expect(
      fill(
        { property_id: 'p-text', operator: 'contains', value: 'pie' },
        { property_id: 'p-text', operator: 'contains', value: 'apple' },
      ),
    ).toEqual({ title: '', values: {} })
  })
})

describe('percent edit text', () => {
  const percent = parseProperty({ id: 'n', name: 'N', type: 'number', config: { format: 'percent' }, position: 0 })

  test('shows and reads the percentage the cell shows', () => {
    expect(numberEditText(percent, 0.125)).toBe('12.5')
    expect(numberEditText(percent, 0.07)).toBe('7')
    expect(numberFromEditText(percent, '50')).toBe(0.5)
    expect(numberFromEditText(percent, ' 12.5% ')).toBe(0.125)
    expect(numberFromEditText(percent, 'abc')).toBe(null)
  })

  test('other formats edit the stored number', () => {
    expect(numberEditText(property('p-num'), 0.5)).toBe('0.5')
    expect(numberFromEditText(property('p-num'), '50')).toBe(50)
    expect(numberFromEditText(property('p-num'), '50%')).toBe(null)
  })
})

test('only rules every row must pass give values: an or group gives nothing, an and group under the root does', () => {
  const status = { property_id: 'p-status', operator: 'is', value: ['s-done'] } as const
  const select = { property_id: 'p-sel', operator: 'is', value: ['o-b'] } as const
  const check = { property_id: 'p-check', operator: 'is', value: true } as const
  expect(
    newRowValues(
      { op: 'and', children: [status, { op: 'or', children: [select, check] }, { op: 'and', children: [check] }] },
      database.properties,
      ctx,
    ),
  ).toEqual({ title: '', values: { 'p-status': 's-done', 'p-check': true } })
  expect(newRowValues({ op: 'or', children: [status, select] }, database.properties, ctx)).toEqual({ title: '', values: {} })
})
