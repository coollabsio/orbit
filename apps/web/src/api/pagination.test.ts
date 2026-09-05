import { expect, test } from 'bun:test'
import { fetchAllPages } from './pagination'

test('fetchAllPages follows every non-null opaque cursor', async () => {
  const cursors: Array<string | undefined> = []
  const page = await fetchAllPages(async (cursor) => {
    cursors.push(cursor)
    if (!cursor) return { items: ['first'], next_cursor: 'opaque-page-two' }
    return { items: ['second'], next_cursor: null }
  })

  expect(cursors).toEqual([undefined, 'opaque-page-two'])
  expect(page).toEqual({ items: ['first', 'second'], next_cursor: null })
})
