import { expect, test } from 'bun:test'
import { taskRedirect } from './taskNavigation'

test('returns a local task redirect with its query string', () => {
  const params = new URLSearchParams('redirect=%2Finbox%3Fworkspace%3Dworkspace-1')
  expect(taskRedirect(params)).toBe('/inbox?workspace=workspace-1')
})

test('rejects external task redirects', () => {
  expect(taskRedirect(new URLSearchParams('redirect=https%3A%2F%2Fevil.example'))).toBeNull()
  expect(taskRedirect(new URLSearchParams('redirect=%2F%2Fevil.example'))).toBeNull()
  expect(taskRedirect(new URLSearchParams('redirect=%2F%5Cevil.example'))).toBeNull()
})
