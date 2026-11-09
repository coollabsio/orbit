import { expect, test } from 'bun:test'
import { database, property } from '@orbit/database/testing'
import type { GroupProperty } from '@orbit/database/types'
import { repairConfig } from '@orbit/database/view'

import { filledProperties, groupValues, listProperties, parseTime, pickView } from './database'

const view = database.views[0]!

test('the list shows the first 3 visible properties of the view after the title, in column order', () => {
  const config = repairConfig(
    {
      ...view,
      config: {
        columns: [
          { property_id: 'p-sel' },
          { property_id: 'p-title' },
          { property_id: 'p-text', hidden: true },
          { property_id: 'p-status' },
          { property_id: 'p-num', hidden: true },
          { property_id: 'p-date' },
        ],
      },
    },
    database.properties,
  )
  expect(listProperties(database.properties, config).map((shown) => shown.id)).toEqual(['p-sel', 'p-status', 'p-date'])
})

test('a row shows only the listed properties it has a value for', () => {
  const shown = [property('p-sel'), property('p-num'), property('p-url')]
  const empty = database.rows.find((row) => row.id === 'r3')!
  const cherry = database.rows.find((row) => row.id === 'r4')!
  expect(filledProperties(empty, shown)).toEqual([])
  expect(filledProperties(cherry, shown).map((shown) => shown.id)).toEqual(['p-sel', 'p-num'])
})

test('moving a card to a column sets the group property, or clears it for "No <property>"', () => {
  const kind = property('p-sel') as GroupProperty
  expect(groupValues(kind, 'o-b')).toEqual({ 'p-sel': 'o-b' })
  expect(groupValues(kind, null)).toEqual({ 'p-sel': null })
})

test('the chosen view while it exists, else the first by position', () => {
  const views = [
    { id: 'b', name: 'Board', type: 'board' as const, config: {}, position: 1 },
    { id: 'a', name: 'Table', type: 'table' as const, config: {}, position: 0 },
  ]
  expect(pickView(views, 'b')?.id).toBe('b')
  expect(pickView(views, 'gone')?.id).toBe('a')
  expect(pickView(views, null)?.id).toBe('a')
})

test('times are HH:MM on a 24 hour clock', () => {
  expect(parseTime('9:05')).toBe('09:05')
  expect(parseTime(' 23:59 ')).toBe('23:59')
  expect(parseTime('24:00')).toBe(null)
  expect(parseTime('9')).toBe(null)
})
