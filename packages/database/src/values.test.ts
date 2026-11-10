import { describe, expect, test } from 'bun:test'
import { ctx, database, properties, property, row } from './testing'
import type { DatabaseProperty } from './types'
import { apiValue, cellOf, displayText, parseDatabase, parseProperty, valueFromText } from './values'

function shown(propertyId: string, rowId: string, context = ctx) {
  const found = database.rows.find((candidate) => candidate.id === rowId)!
  return displayText(property(propertyId), cellOf(found, property(propertyId)), context)
}

function numberText(config: Record<string, unknown>, value: number) {
  const number = parseProperty({ id: 'n', name: 'N', type: 'number', config, position: 0 })
  return displayText(number, { kind: 'number', number: value }, ctx)
}

function dateText(config: Record<string, unknown>, start: string, end: string | null = null) {
  const date = parseProperty({ id: 'd', name: 'D', type: 'date', config, position: 0 })
  return displayText(date, valueFromText(date, end ? `${start}/${end}` : start, ctx), ctx)
}

describe('display text', () => {
  test('options, people and computed values', () => {
    expect(shown('p-sel', 'r1')).toBe('Zeta')
    expect(shown('p-multi', 'r1')).toBe('x-ray, yankee')
    expect(shown('p-status', 'r2')).toBe('In progress')
    expect(shown('p-status', 'r3')).toBe('Not started')
    expect(shown('p-person', 'r2')).toBe('Bob Stone, Cy Young')
    expect(shown('p-cb', 'r3')).toBe('Cy Young')
    expect(shown('p-lt', 'r1')).toBe('Nov 8, 2026 23:30')
    expect(shown('p-title', 'r3')).toBe('')
    expect(shown('p-num', 'r3')).toBe('')
  })

  test('number formats', () => {
    expect(numberText({ format: 'number' }, 1234.5)).toBe('1234.5')
    expect(numberText({ format: 'number_with_commas' }, 1234.5)).toBe('1,234.5')
    expect(numberText({ format: 'percent' }, 0.125)).toBe('12.5%')
    expect(numberText({ format: 'percent' }, 0.07)).toBe('7%')
    expect(numberText({ format: 'currency' }, 1234.5)).toBe('$1,234.50')
    expect(numberText({ format: 'currency', currency: 'EUR' }, -3)).toBe('-€3.00')
  })

  test('date formats, in the viewer zone (New York) for times', () => {
    expect(dateText({ date_format: 'short' }, '2026-11-09')).toBe('Nov 9, 2026')
    expect(dateText({ date_format: 'iso' }, '2026-11-09')).toBe('2026-11-09')
    expect(dateText({ date_format: 'relative' }, '2026-11-09')).toBe('Today')
    expect(dateText({ date_format: 'relative' }, '2026-11-10')).toBe('Tomorrow')
    expect(dateText({ date_format: 'relative' }, '2026-11-08')).toBe('Yesterday')
    expect(dateText({ date_format: 'relative' }, '2026-11-20')).toBe('Nov 20, 2026')
    expect(dateText({ time_format: '24h' }, '2026-11-10T03:00:00Z')).toBe('Nov 9, 2026 22:00')
    expect(dateText({ time_format: '12h' }, '2026-11-10T03:00:00Z')).toBe('Nov 9, 2026 10:00 PM')
    expect(dateText({ date_format: 'iso' }, '2026-07-01T00:05:00Z')).toBe('2026-06-30 20:05')
    expect(dateText({}, '2026-11-02', '2026-11-12')).toBe('Nov 2, 2026 → Nov 12, 2026')
  })
})

describe('values from the API', () => {
  test('values that do not fit the property are empty', () => {
    const parsed = parseDatabase({
      database: { id: 'db', title: 'Bad', icon: null, locked: false, row_count: 1 },
      properties,
      views: [],
      rows: [
        row('bad', 0, 'Bad', {
          'p-text': '',
          'p-num': '12',
          'p-sel': 'deleted-option',
          'p-multi': ['t-x', 't-x'],
          'p-date': { start: '2026-11-09', end: '2026-11-01' },
          'p-person': [],
          'p-check': false,
          'p-url': 'ftp://example.com',
          'p-title': 'not a stored value',
          unknown: 'x',
        }),
      ],
    })
    expect(parsed.rows[0]!.values).toEqual({})
  })

  test('a property config with missing fields gets the server defaults', () => {
    const raw: DatabaseProperty = {
      id: 's',
      name: 'S',
      type: 'status',
      config: { options: [{ id: 'a', name: 'A' }, { name: 'no id' }, { id: 'b', name: 'B', color: 'teal', group: 'done' }] },
      position: 0,
    }
    expect(parseProperty(raw)).toEqual({
      id: 's',
      name: 'S',
      type: 'status',
      position: 0,
      config: {
        options: [
          { id: 'a', name: 'A', color: 'default', group: 'todo' },
          { id: 'b', name: 'B', color: 'default', group: 'done' },
        ],
      },
    })
  })
})

describe('values from pasted text follow the server conversion rules', () => {
  const paste = (propertyId: string, text: string) => apiValue(valueFromText(property(propertyId), text, ctx))

  test.each([
    ['p-text', '  hello  ', 'hello'],
    ['p-text', '   ', null],
    ['p-num', '1e3', 1000],
    ['p-num', ' -2.5 ', -2.5],
    ['p-num', '1,000', null],
    ['p-num', '0x10', null],
    ['p-num', '', null],
    ['p-check', 'YES', true],
    ['p-check', 'x', true],
    ['p-check', 'no', null],
    ['p-url', 'https://orbit.dev/a', 'https://orbit.dev/a'],
    ['p-url', 'ftp://orbit.dev', null],
    ['p-url', 'https://orbit .dev', null],
    ['p-date', '2026-11-09', { start: '2026-11-09', end: null }],
    ['p-date', '2026-11-09T14:00Z', { start: '2026-11-09T14:00:00Z', end: null }],
    ['p-date', '2026-11-09T14:00:30.25Z', { start: '2026-11-09T14:00:30Z', end: null }],
    ['p-date', '2026-11-09 / 2026-11-12', { start: '2026-11-09', end: '2026-11-12' }],
    ['p-date', '2026-11-12/2026-11-09', null],
    ['p-date', '2026-11-09/2026-11-12T10:00Z', null],
    ['p-date', '2026-02-29', null],
    ['p-date', '2026-11-09T14:00', null],
    ['p-sel', ' alpha ', 'o-b'],
    ['p-sel', 'unknown', null],
    ['p-status', 'DONE', 's-done'],
    ['p-multi', 'Yankee, nope, zulu, yankee', ['t-y', 't-z']],
    ['p-person', 'bob stone, Ada Lovelace', ['u-bob', 'u-me']],
    ['p-person', 'nobody', null],
    ['p-title', 'Title', null],
    ['p-ct', '2026-11-09', null],
  ] as const)('%s from %p', (propertyId, text, expected) => {
    expect(paste(propertyId, text)).toEqual(expected)
  })

  test('text is cut to 10,000 characters', () => {
    const value = valueFromText(property('p-text'), '😀'.repeat(10_001), ctx)
    expect(value).toEqual({ kind: 'text', text: '😀'.repeat(10_000) })
  })
})
