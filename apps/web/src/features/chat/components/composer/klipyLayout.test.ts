import { expect, test } from 'bun:test'
import { masonryColumns } from './klipyLayout'

const item = (id: string, width: number, height: number) => ({ id, preview: { url: `https://static.klipy.com/${id}.gif`, width, height } })

test('each item goes into the shortest column, by its height relative to its width', () => {
  const items = [item('tall', 100, 300), item('a', 100, 100), item('b', 100, 100), item('c', 100, 100), item('d', 100, 100)]
  expect(masonryColumns(items, 2).map((column) => column.map((each) => each.id))).toEqual([['tall', 'd'], ['a', 'b', 'c']])
})

test('a next page leaves the items of the first where they were', () => {
  const first = [item('a', 200, 100), item('b', 100, 200), item('c', 100, 100)]
  const before = masonryColumns(first, 2)
  const after = masonryColumns([...first, item('d', 100, 100)], 2)
  expect(after.map((column) => column.slice(0, before[after.indexOf(column)].length))).toEqual(before)
  // an image without a size counts as a square
  expect(masonryColumns([item('z', 0, 0)], 2)).toEqual([[item('z', 0, 0)], []])
})
