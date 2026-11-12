import { expect, test } from 'bun:test'
import { pullFromSearch, withoutPull, withPull } from './overlay'

const ref = { owner: 'acme', repo: 'orbit', number: 482 }

test('a pull request opens over the page and keeps the page’s own search', () => {
  const search = withPull('?view=board', ref, 9001)
  expect(search).toBe('?view=board&pr=acme/orbit/482&prComment=9001')
  expect(pullFromSearch(search)).toEqual({ ref, commentId: 9001 })
  expect(withoutPull(search)).toBe('?view=board')
})

test('opening another pull request drops the old comment, and closing leaves no empty search', () => {
  const search = withPull(withPull('', ref, 9001), { ...ref, number: 7 })
  expect(pullFromSearch(search)).toEqual({ ref: { ...ref, number: 7 }, commentId: null })
  expect(withoutPull(search)).toBe('')
})

test('a broken pr parameter opens nothing', () => {
  expect(pullFromSearch('?pr=acme%2F..%2F1')).toBeNull()
  expect(pullFromSearch('?pr=acme/orbit/x')).toBeNull()
})
