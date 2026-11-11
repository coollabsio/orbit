import { describe, expect, test } from 'bun:test'
import {
  csvTable,
  guardCell,
  guessTargets,
  guessTitleColumn,
  guessType,
  importColumns,
  mappingProblem,
  parseCsv,
  retarget,
  toCsv,
  unguard,
  viewCsv,
  viewCsvName,
  viewProperties,
} from './csv'
import { ctx, properties, rows } from './testing'
import type { DatabaseView } from './types'
import { parseDatabase } from './values'

const database = parseDatabase({
  database: { id: 'db', title: 'Recipes', icon: null, locked: false, row_count: rows.length },
  properties,
  views: [],
  rows,
} as never)

function view(type: 'table' | 'board', config: Record<string, unknown>): DatabaseView {
  return { id: 'v', name: 'View', type, position: 0, config } as DatabaseView
}

describe('parseCsv', () => {
  test('reads quotes, escaped quotes, line breaks in fields and a BOM', () => {
    expect(parseCsv('﻿Name,Notes\r\n"Pie, apple","say ""hi""\nthen go"\r\nTart,\n')).toEqual([
      ['Name', 'Notes'],
      ['Pie, apple', 'say "hi"\nthen go'],
      ['Tart', ''],
    ])
  })

  test('detects a semicolon or tab delimiter from the first line', () => {
    expect(parseCsv('a;b;"c,d"\n1;2;3')).toEqual([
      ['a', 'b', 'c,d'],
      ['1', '2', '3'],
    ])
    expect(parseCsv('a\tb\n1\t2')).toEqual([
      ['a', 'b'],
      ['1', '2'],
    ])
  })

  test('drops blank lines and keeps ragged rows', () => {
    expect(parseCsv('a,b\n\n1\r\r\n2,3,4')).toEqual([['a', 'b'], ['1'], ['2', '3', '4']])
  })

  test('round trips what toCsv writes', () => {
    const records = [
      ['Name', 'Notes'],
      ['Crème brûlée', 'line one\r\nline "two"'],
      ['a;b', ''],
    ]
    const text = toCsv(records)
    expect(text.startsWith('﻿')).toBe(true)
    expect(text.endsWith('\r\n')).toBe(true)
    expect(parseCsv(text)).toEqual(records)
  })
})

describe('guessType', () => {
  test('guesses from the non-empty values', () => {
    expect(guessType(['1', '-2.5', '', '1e3'])).toBe('number')
    expect(guessType(['yes', 'No', 'x', ''])).toBe('checkbox')
    expect(guessType(['2026-11-09', '2026-11-10T14:00:00Z'])).toBe('date')
    expect(guessType(['https://a.example', 'http://b.example/x'])).toBe('url')
    expect(guessType(['Open', 'Closed', 'Open'])).toBe('select')
    expect(guessType(['Ada', 'Grace'])).toBe('text')
    expect(guessType(['', ' '])).toBe('text')
    expect(guessType(['1', 'two'])).toBe('text')
  })

  test('a select has at most 50 distinct short values', () => {
    const many = Array.from({ length: 51 }, (_, index) => `v${index}`)
    expect(guessType([...many, ...many])).toBe('text')
    expect(guessType(['a'.repeat(41), 'a'.repeat(41)])).toBe('text')
  })

  test('the title column is the one named like a title', () => {
    expect(guessTitleColumn(['Id', ' Title ', 'Name'])).toBe(1)
    expect(guessTitleColumn(['Id', 'Body'])).toBe(0)
  })
})

describe('formula injection guard', () => {
  test('prefixes text that a spreadsheet would run, never numbers', () => {
    expect(['=1+1', '+1', '-x', '@SUM(A1)', '\tx', '\rx', 'safe'].map((text) => guardCell(text, false))).toEqual([
      "'=1+1",
      "'+1",
      "'-x",
      "'@SUM(A1)",
      "'\tx",
      "'\rx",
      'safe',
    ])
    expect(guardCell('-2.5', true)).toBe('-2.5')
    expect(unguard("'=1+1")).toBe('=1+1')
    expect(unguard("'quoted")).toBe("'quoted")
  })
})

describe('viewCsv', () => {
  test('writes the visible columns in view order with machine formats', () => {
    const table = view('table', {
      columns: [
        { property_id: 'p-title' },
        { property_id: 'p-num' },
        { property_id: 'p-text', hidden: true },
        { property_id: 'p-multi' },
        { property_id: 'p-status' },
        { property_id: 'p-date' },
        { property_id: 'p-person' },
        { property_id: 'p-check' },
        { property_id: 'p-ct' },
        ...['p-sel', 'p-url', 'p-cb', 'p-lt', 'p-lb'].map((id) => ({ property_id: id, hidden: true })),
      ],
    })
    const shown = viewProperties(database, table)
    expect(shown.map((property) => property.name)).toEqual([
      'Name',
      'Score',
      'Tags',
      'Status',
      'Due',
      'Owner',
      'Done?',
      'Created',
    ])
    const withFormula = { ...database.rows[2], title: '=HYPERLINK("x")' }
    expect(parseCsv(viewCsv(shown, [database.rows[0], database.rows[1], withFormula], ctx))).toEqual([
      ['Name', 'Score', 'Tags', 'Status', 'Due', 'Owner', 'Done?', 'Created'],
      ['Apple pie', '10', 'x-ray, yankee', 'Done', '2026-11-09', 'Ada Lovelace', 'true', '2026-11-01T12:00:00.000Z'],
      ['banana', '-2.5', 'zulu', 'In progress', '2026-11-10T03:00:00Z', 'Bob Stone, Cy Young', 'false', '2026-10-09T12:00:00.000Z'],
      ["'=HYPERLINK(\"x\")", '', '', 'Not started', '', '', 'false', '2026-11-08T12:00:00.000Z'],
    ])
  })

  test('a board writes the title and its card properties', () => {
    const board = view('board', { group_by: 'p-status', card_properties: ['p-num', 'p-sel'] })
    expect(viewProperties(database, board).map((property) => property.name)).toEqual(['Name', 'Score', 'Kind'])
  })
})

describe('import mapping', () => {
  const table = csvTable(
    parseCsv('Id,Title,Score,Kind,Created\n1,Pie,10,Zeta,x\n2,Tart,-1,"\'=bad",y\n3,Cake,,Zeta,z'),
    true,
  )

  test('a table names empty headers and undoes export guards', () => {
    expect(csvTable([['a', ''], ['1', '2', '3']], true)).toEqual({ header: ['a', 'Column 2', 'Column 3'], rows: [['1', '2', '3']] })
    expect(csvTable([['x']], false)).toEqual({ header: ['Column 1'], rows: [['x']] })
    expect(table.rows[1][3]).toBe('=bad')
  })

  test('a new database takes the title column by name and guesses the rest', () => {
    expect(guessTargets(table, null)).toEqual([
      { kind: 'new', type: 'number' },
      { kind: 'new', type: 'title' },
      { kind: 'new', type: 'number' },
      { kind: 'new', type: 'select' },
      { kind: 'new', type: 'text' },
    ])
  })

  test('a merge matches property names, never a computed property', () => {
    expect(guessTargets(table, database.properties)).toEqual([
      { kind: 'new', type: 'number' },
      { kind: 'property', propertyId: 'p-title' },
      { kind: 'property', propertyId: 'p-num' },
      { kind: 'property', propertyId: 'p-sel' },
      { kind: 'new', type: 'text' },
    ])
  })

  test('moving the title or a property frees the column that had it', () => {
    const targets = guessTargets(table, database.properties)
    const moved = retarget(table, targets, 0, { kind: 'property', propertyId: 'p-title' }, database.properties)
    expect(moved[0]).toEqual({ kind: 'property', propertyId: 'p-title' })
    expect(moved[1]).toEqual({ kind: 'new', type: 'text' })
    const again = retarget(table, moved, 4, { kind: 'property', propertyId: 'p-num' }, database.properties)
    expect(again[2]).toEqual({ kind: 'new', type: 'number' })
    expect(mappingProblem(table, again.map(() => ({ kind: 'skip' })), database.properties)).toBe('Choose the column that holds the titles.')
    expect(mappingProblem(table, again, database.properties)).toBeNull()
  })

  test('request columns name each target', () => {
    expect(importColumns(table, [{ kind: 'skip' }, { kind: 'new', type: 'title' }, { kind: 'property', propertyId: 'p-num' }])).toEqual([
      { name: 'Id', target: 'skip' },
      { name: 'Title', target: { new: 'title' } },
      { name: 'Score', target: { property_id: 'p-num' } },
      { name: 'Kind', target: 'skip' },
      { name: 'Created', target: 'skip' },
    ])
  })
})

test('a view CSV is named after the database and the view', () => {
  expect(viewCsvName({ ...database, database: { ...database.database, title: 'Q3: plans/ideas' } }, view('table', {}))).toBe('Q3_ plans_ideas - View.csv')
})
