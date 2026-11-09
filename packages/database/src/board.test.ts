import { describe, expect, test } from 'bun:test'
import { cardAfter, groupRows, type GroupProperty } from './board'
import { database, property } from './testing'

const select = property('p-sel') as GroupProperty
const status = property('p-status') as GroupProperty
const columns = (groups: ReturnType<typeof groupRows>) => groups.map((group) => [group.id, group.rows.map((row) => row.id), group.hidden])

describe('groupRows', () => {
  test('a select board shows "No value" first, then the options in option order', () => {
    expect(columns(groupRows(database.rows, select, { group_order: [], hidden_groups: [] }))).toEqual([
      [null, ['r3'], false],
      ['o-a', ['r1'], false],
      ['o-b', ['r2'], false],
      ['o-c', ['r4'], false],
    ])
  })

  test('group_order leads, missing options follow, hidden groups stay in place', () => {
    expect(columns(groupRows(database.rows, select, { group_order: ['o-c', null, 'gone'], hidden_groups: ['o-a'] }))).toEqual([
      ['o-c', ['r4'], false],
      [null, ['r3'], false],
      ['o-a', ['r1'], true],
      ['o-b', ['r2'], false],
    ])
  })

  test('a row with no status sits in the first todo option; a status board has no "No value" column', () => {
    expect(columns(groupRows(database.rows, status, { group_order: [null, 's-done'], hidden_groups: [] }))).toEqual([
      ['s-done', ['r1'], false],
      ['s-todo', ['r3', 'r4'], false],
      ['s-prog', ['r2'], false],
    ])
  })

  test('a value naming a deleted option counts as no value, and rows keep their order', () => {
    const rows = [...database.rows].reverse().map((row) => (row.id === 'r1' ? { ...row, values: { 'p-sel': { kind: 'choice' as const, id: 'gone' } } } : row))
    expect(groupRows(rows, select, { group_order: [], hidden_groups: [] })[0]!.rows.map((row) => row.id)).toEqual(['r3', 'r1'])
  })
})

describe('cardAfter', () => {
  const ordered = database.rows
  const row = (id: string) => ordered.find((candidate) => candidate.id === id)!

  test('below a card: right after it', () => {
    expect(cardAfter(ordered, [row('r1'), row('r4')], 1, 'r2')).toBe('r1')
  })

  test('at the top of a column: right before its first card', () => {
    expect(cardAfter(ordered, [row('r3'), row('r4')], 0, 'r1')).toBe('r2')
    expect(cardAfter(ordered, [row('r2')], 0, 'r1')).toBeNull()
    expect(cardAfter(ordered, [row('r1')], 0, 'r4')).toBeNull()
  })

  test('in an empty column: the card keeps its place', () => {
    expect(cardAfter(ordered, [], 0, 'r3')).toBe('r2')
    expect(cardAfter(ordered, [], 0, 'r1')).toBeNull()
  })
})
