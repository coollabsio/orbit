import { expect, test } from 'bun:test'
import { keepIdentifiersTogether } from './toast'

test('identifiers in toast text cannot break after their hyphen; other words are untouched', () => {
  expect(keepIdentifiersTogether('GEN-0D6F is now a sub-issue of GEN-C68D')).toBe('GEN-\u20600D6F is now a sub-issue of GEN-\u2060C68D')
  expect(keepIdentifiersTogether('Closed 2 sub-issues and parent ORB-12')).toBe('Closed 2 sub-issues and parent ORB-\u206012')
  expect(keepIdentifiersTogether('2 tasks are no longer sub-issues')).toBe('2 tasks are no longer sub-issues')
})
