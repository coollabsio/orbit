import { expect, test } from 'bun:test'
import { matchesSearch } from './SearchPicker'

test('matches every word in any of the texts, case-insensitive', () => {
  expect(matchesSearch('', ['acme/api'])).toBe(true)
  expect(matchesSearch('ACME/a', ['acme/api'])).toBe(true)
  expect(matchesSearch('acme api', ['acme/api'])).toBe(true)
  expect(matchesSearch('ada example', ['Ada Lovelace', 'ada@example.com'])).toBe(true)
  expect(matchesSearch('ada bob', ['Ada Lovelace', 'ada@example.com'])).toBe(false)
})
