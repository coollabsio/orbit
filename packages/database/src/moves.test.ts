import { describe, expect, test } from 'bun:test'
import { droppedValuesText, moveDestinations, type MovablePage } from './moves'

const pages: MovablePage[] = [
  { id: 'home', parent_id: null, kind: 'page' },
  { id: 'notes', parent_id: 'home', kind: 'page' },
  { id: 'deep', parent_id: 'notes', kind: 'page' },
  { id: 'tasks', parent_id: null, kind: 'database' },
  { id: 'inner', parent_id: 'home', kind: 'database' },
  { id: 'other', parent_id: null, kind: 'page' },
]

const ids = (list: MovablePage[]) => list.map((page) => page.id)

describe('moveDestinations', () => {
  test('a page can go under any page or database outside its own subtree, except its parent', () => {
    expect(ids(moveDestinations(pages, pages[1]!))).toEqual(['tasks', 'inner', 'other'])
  })

  test('a database cannot become a row', () => {
    expect(ids(moveDestinations(pages, pages[4]!))).toEqual(['notes', 'deep', 'other'])
  })

  test('a row (not in the tree) can go anywhere but its own database', () => {
    expect(ids(moveDestinations(pages, { id: 'row', parent_id: 'tasks', kind: 'page' }))).toEqual(['home', 'notes', 'deep', 'inner', 'other'])
  })
})

test('droppedValuesText counts values', () => {
  expect(droppedValuesText(1)).toBe('1 property value will be removed.')
  expect(droppedValuesText(3)).toBe('3 property values will be removed.')
})
