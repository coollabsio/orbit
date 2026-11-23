import { expect, test } from 'bun:test'

import { extractLinkUrls, linkTaskIds, orbitLinkTarget } from './link-cards'

test('card URLs skip code and angle brackets, drop trailing punctuation, and stop at three', () => {
  expect(extractLinkUrls('see https://a.test/x. and `https://code.test` and <https://quiet.test>')).toEqual(['https://a.test/x'])
  expect(extractLinkUrls('https://a.test https://b.test https://a.test https://c.test https://d.test')).toEqual(['https://a.test', 'https://b.test', 'https://c.test'])
})

test('typed task identifiers become task URLs, not escaped ones or other words', () => {
  const origin = 'https://orbit.test'
  expect(linkTaskIds('fix eng-12 now, not \\ENG-13 or UTF-8', ['ENG'], origin)).toBe('fix https://orbit.test/tasks/ENG-12 now, not \\ENG-13 or UTF-8')
  expect(extractLinkUrls(linkTaskIds('ENG-12', ['ENG'], origin))).toEqual(['https://orbit.test/tasks/ENG-12'])
})

test('Orbit URLs of this server name a task, page or view; other URLs name nothing', () => {
  const origin = 'https://orbit.test'
  expect(orbitLinkTarget('https://orbit.test/tasks/ENG-12', origin)).toEqual({ kind: 'task', id: 'ENG-12' })
  expect(orbitLinkTarget('https://orbit.test/views/v1/ENG-3', origin)).toEqual({ kind: 'task', id: 'ENG-3' })
  expect(orbitLinkTarget('https://orbit.test/docs/p1', origin)).toEqual({ kind: 'page', id: 'p1' })
  expect(orbitLinkTarget('https://orbit.test/views/v1', origin)).toEqual({ kind: 'view', id: 'v1' })
  expect(orbitLinkTarget('https://orbit.test/docs/trash', origin)).toBeNull()
  expect(orbitLinkTarget('https://other.test/tasks/ENG-12', origin)).toBeNull()
})
