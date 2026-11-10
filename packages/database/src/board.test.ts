import { describe, expect, test } from 'bun:test'
import { cardAfter, groupName, groupRows, groupValues, laneRows, moveValues } from './board'
import { ctx, database, property } from './testing'
import type { GroupProperty, LaneProperty, Row } from './types'

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

const person = property('p-person') as GroupProperty
const multi = property('p-multi') as GroupProperty
const checkbox = property('p-check') as LaneProperty
const none = { group_order: [], hidden_groups: [] }
const byId = (id: string) => database.rows.find((row) => row.id === id)!
const withValues = (id: string, values: Row['values']): Row => ({ ...byId(id), values })

describe('groupRows by several values', () => {
  test('a row with two people is a card in both columns; people sort by name, "No value" last', () => {
    expect(columns(groupRows(database.rows, person, none, ctx.members))).toEqual([
      ['u-me', ['r1'], false],
      ['u-bob', ['r2'], false],
      ['u-cy', ['r2'], false],
      [null, ['r3', 'r4'], false],
    ])
  })

  test('group_order places people (and "No value") first; unknown users sort after the named ones', () => {
    const rows = [...database.rows, withValues('r3', { 'p-person': { kind: 'people', ids: ['u-gone'] } })]
    expect(columns(groupRows(rows, person, { group_order: ['u-cy', null], hidden_groups: ['u-me'] }, ctx.members))).toEqual([
      ['u-cy', ['r2'], false],
      [null, ['r3', 'r4'], false],
      ['u-me', ['r1'], true],
      ['u-bob', ['r2'], false],
      ['u-gone', ['r3'], false],
    ])
  })

  test('a multi-select board shows every option, as a select', () => {
    expect(columns(groupRows(database.rows, multi, none))).toEqual([
      [null, ['r3', 'r4'], false],
      ['t-x', ['r1'], false],
      ['t-y', ['r1'], false],
      ['t-z', ['r2'], false],
    ])
  })

  test('hide_empty_groups leaves out the columns with no cards', () => {
    const rows = database.rows.filter((row) => row.id !== 'r2')
    expect(columns(groupRows(rows, multi, { ...none, hide_empty_groups: true }))).toEqual([
      [null, ['r3', 'r4'], false],
      ['t-x', ['r1'], false],
      ['t-y', ['r1'], false],
    ])
  })
})

describe('laneRows', () => {
  const lanes = (config: Partial<Parameters<typeof laneRows>[3]> = {}) =>
    laneRows(database.rows, select, checkbox, { ...none, sub_group_order: [], hidden_sub_groups: [], ...config }).map((lane) => [lane.id, columns(lane.columns)])

  test('each lane has the same columns, with only its own cards', () => {
    expect(lanes()).toEqual([
      ['false', [[null, ['r3'], false], ['o-a', [], false], ['o-b', ['r2'], false], ['o-c', ['r4'], false]]],
      ['true', [[null, [], false], ['o-a', ['r1'], false], ['o-b', [], false], ['o-c', [], false]]],
    ])
  })

  test('sub_group_order leads; hide_empty_groups drops empty lanes and columns empty in every lane', () => {
    const rows = database.rows.filter((row) => row.id !== 'r1')
    const config = { ...none, sub_group_order: ['true'], hidden_sub_groups: [], hide_empty_groups: true }
    expect(laneRows(rows, select, checkbox, config).map((lane) => [lane.id, columns(lane.columns)])).toEqual([
      ['false', [[null, ['r3'], false], ['o-b', ['r2'], false], ['o-c', ['r4'], false]]],
    ])
    expect(lanes({ sub_group_order: ['true'], hidden_sub_groups: ['false'] }).map(([id]) => id)).toEqual(['true', 'false'])
  })
})

describe('moveValues', () => {
  test('a card moved from one person to another keeps the other people', () => {
    expect(moveValues(byId('r2'), person, 'u-bob', 'u-me')).toEqual({ 'p-person': ['u-cy', 'u-me'] })
    expect(moveValues(byId('r2'), person, 'u-bob', 'u-cy')).toEqual({ 'p-person': ['u-cy'] })
  })

  test('a drop on "No value" clears the value; a drop from it sets one value; the same group is no patch', () => {
    expect(moveValues(byId('r1'), multi, 't-x', null)).toEqual({ 'p-multi': null })
    expect(moveValues(byId('r3'), multi, null, 't-z')).toEqual({ 'p-multi': ['t-z'] })
    expect(moveValues(byId('r1'), multi, 't-x', 't-x')).toEqual({})
  })

  test('a select sets the option and a checkbox lane sets the box', () => {
    expect(moveValues(byId('r1'), select, 'o-a', null)).toEqual({ 'p-sel': null })
    expect(moveValues(byId('r1'), checkbox, 'true', 'false')).toEqual({ 'p-check': false })
  })
})

describe('groupValues and groupName', () => {
  test('a new card in a group gets its value; "No <property>" none', () => {
    expect(groupValues(select, 'o-b')).toEqual({ 'p-sel': 'o-b' })
    expect(groupValues(select, null)).toEqual({ 'p-sel': null })
    expect(groupValues(person, 'u-bob')).toEqual({ 'p-person': ['u-bob'] })
    expect(groupValues(checkbox, 'true')).toEqual({ 'p-check': true })
  })

  test('a group is named by its option, member or box', () => {
    const [people, multis, lanes] = [groupRows(database.rows, person, none, ctx.members), groupRows(database.rows, multi, none), laneRows(database.rows, select, checkbox, { ...none, sub_group_order: [], hidden_sub_groups: [] })]
    expect(people.map((group) => groupName(person, group, ctx.members))).toEqual(['Ada Lovelace', 'Bob Stone', 'Cy Young', 'No Owner'])
    expect(groupName(person, { id: 'u-gone', option: null }, ctx.members)).toBe('Former member')
    expect(multis.map((group) => groupName(multi, group, ctx.members))).toEqual(['No Tags', 'x-ray', 'yankee', 'zulu'])
    expect(lanes.map((lane) => groupName(checkbox, lane, ctx.members))).toEqual(['Unchecked', 'Checked'])
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
