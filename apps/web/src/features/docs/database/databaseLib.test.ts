import { describe, expect, test } from 'bun:test'
import { ctx, database } from '@orbit/database/testing'
import { applyView } from '@orbit/database/view'
import { neighborRows, withPinnedRows } from './databaseLib'

describe('neighborRows', () => {
  // Score >= 0, highest first: r1 (10), r4 (0). r2 (-2.5) and r3 (no score) are filtered out; r3 was just added at the top.
  const view = {
    ...database.views[0],
    config: { filter: { rules: [{ property_id: 'p-num', operator: '>=' as const, value: 0 }] }, sorts: [{ property_id: 'p-num', direction: 'desc' as const }] },
  }
  const order = withPinnedRows(applyView(database, view, '', ctx), [{ id: 'r3', at: 'top' }], database.rows).map((row) => row.id)

  test('previous and next follow the view order, with filter, sort and new rows', () => {
    expect(order).toEqual(['r3', 'r1', 'r4'])
    expect(neighborRows(order, 'r3')).toEqual({ previous: null, next: 'r1' })
    expect(neighborRows(order, 'r1')).toEqual({ previous: 'r3', next: 'r4' })
    expect(neighborRows(order, 'r4')).toEqual({ previous: 'r1', next: null })
  })

  test('a row the view does not show has no neighbors', () => {
    expect(neighborRows(order, 'r2')).toEqual({ previous: null, next: null })
  })
})
