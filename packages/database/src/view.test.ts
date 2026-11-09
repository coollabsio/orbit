import { describe, expect, test } from 'bun:test'
import { ctx, database } from './testing'
import type { DatabaseView, DatabaseViewConfig } from './types'
import { applyView, repairConfig } from './view'

function view(config: DatabaseViewConfig): DatabaseView {
  return { id: 'v', name: 'Table', type: 'table', config, position: 0 }
}

const short = database.properties.filter((property) => ['p-title', 'p-text', 'p-num'].includes(property.id))

describe('repairConfig', () => {
  test('drops deleted and repeated entries, fills defaults, appends missing columns', () => {
    const repaired = repairConfig(
      view({
        columns: [
          { property_id: 'p-num', width: 120, hidden: true },
          { property_id: 'gone', width: 90 },
          { property_id: 'p-num', width: 300 },
        ],
        sorts: [
          { property_id: 'gone', direction: 'asc' },
          { property_id: 'p-num', direction: 'desc' },
          { property_id: 'p-num', direction: 'asc' },
        ],
        filter: {
          rules: [
            { property_id: 'gone', operator: 'contains', value: 'x' },
            { property_id: 'p-text', operator: 'contains', value: 'a' },
            { property_id: 'p-text', operator: 'not_contains', value: 'b' },
          ],
        },
      }),
      short,
    )
    expect(repaired).toEqual({
      columns: [
        { property_id: 'p-num', width: 120, hidden: true },
        { property_id: 'p-title', width: 280, hidden: false },
        { property_id: 'p-text', width: 200, hidden: false },
      ],
      sorts: [{ property_id: 'p-num', direction: 'desc' }],
      filter: {
        rules: [
          { property_id: 'p-text', operator: 'contains', value: 'a' },
          { property_id: 'p-text', operator: 'not_contains', value: 'b' },
        ],
      },
      wrap_cells: false,
    })
  })

  test('an empty config shows every property in property order', () => {
    expect(repairConfig(view({ wrap_cells: true }), short)).toEqual({
      columns: [
        { property_id: 'p-title', width: 280, hidden: false },
        { property_id: 'p-text', width: 200, hidden: false },
        { property_id: 'p-num', width: 200, hidden: false },
      ],
      sorts: [],
      filter: { rules: [] },
      wrap_cells: true,
    })
  })
})

describe('applyView', () => {
  const ids = (config: DatabaseViewConfig, search = '') =>
    applyView(database, view(config), search, ctx).map((row) => row.id)

  test('without a config it shows every row in manual order', () => {
    expect(ids({})).toEqual(['r1', 'r2', 'r3', 'r4'])
  })

  test('search covers the title and the display text of visible columns, ignoring case', () => {
    expect(ids({}, 'CRUST')).toEqual(['r1', 'r2'])
    expect(ids({}, '  apple ')).toEqual(['r1'])
    expect(ids({}, 'zeta')).toEqual(['r1'])
    expect(ids({}, 'cy young')).toEqual(['r2', 'r3'])
    expect(ids({}, 'nov 9, 2026')).toEqual(['r1', 'r2'])
    expect(ids({}, 'not started')).toEqual(['r3', 'r4'])
    expect(ids({}, '   ')).toEqual(['r1', 'r2', 'r3', 'r4'])
  })

  test('search skips hidden columns but never the title', () => {
    const hidden = { columns: [{ property_id: 'p-text', hidden: true }, { property_id: 'p-title', hidden: true }] }
    expect(ids(hidden, 'crust')).toEqual([])
    expect(ids(hidden, 'banana')).toEqual(['r2'])
  })

  test('filters, then search, then sorts', () => {
    expect(
      ids(
        {
          filter: {
            rules: [
              { property_id: 'p-num', operator: '<=', value: 10 },
              { property_id: 'p-text', operator: 'is_not_empty' },
              { property_id: 'gone', operator: 'is_empty' },
            ],
          },
          sorts: [{ property_id: 'p-num', direction: 'desc' }],
        },
        'r',
      ),
    ).toEqual(['r1', 'r4', 'r2'])
  })
})
