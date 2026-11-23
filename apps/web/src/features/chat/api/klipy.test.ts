import { expect, test } from 'bun:test'
import type { KlipyItem } from '@orbit/api/generated/types.gen'
import { klipyItems, nextKlipyPage } from './klipy'

const item = (id: string): KlipyItem => {
  const image = { url: `https://static.klipy.com/${id}.gif`, width: 200, height: 150 }
  return { id, title: id, kind: 'gif', preview: image, full: image }
}

test('the next page follows while KLIPY has one, up to the last page the server serves', () => {
  expect(nextKlipyPage({ items: [], has_next: true }, 1)).toBe(2)
  expect(nextKlipyPage({ items: [], has_next: false }, 1)).toBeUndefined()
  expect(nextKlipyPage({ items: [], has_next: true }, 50)).toBeUndefined()
})

test('the items of all pages show in order, each once', () => {
  const pages = [
    { items: [item('a'), item('b')], has_next: true },
    // trending moved: `b` is on the second page too
    { items: [item('b'), item('c')], has_next: false },
  ]
  expect(klipyItems(pages).map((each) => each.id)).toEqual(['a', 'b', 'c'])
  expect(klipyItems(undefined)).toEqual([])
})
