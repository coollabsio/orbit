import { expect, test } from 'bun:test'
import { faviconHref } from './favicon'

test('the tab icon has the dot only while something is unread', () => {
  expect(faviconHref(0)).toBe('/favicon.svg')
  expect(faviconHref(1)).toBe('/favicon-unread.svg')
  expect(faviconHref(120)).toBe('/favicon-unread.svg')
})
