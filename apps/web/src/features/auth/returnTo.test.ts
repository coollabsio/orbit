import { expect, test } from 'bun:test'
import { loginDestination, loginPath } from './returnTo'

test('a sign-in goes back to the Orbit page that asked for it', () => {
  expect(loginDestination('/tasks/abc?view=list#comment-1')).toBe('/tasks/abc?view=list#comment-1')
  expect(loginPath('/docs/page-1?x=1')).toBe('/login?return_to=%2Fdocs%2Fpage-1%3Fx%3D1')
  expect(loginPath('/')).toBe('/login')
  expect(loginPath('/login?return_to=%2Ftasks')).toBe('/login')
})

test('a sign-in never leaves Orbit or goes back to a sign-in page', () => {
  for (const input of [null, '', 'tasks', '//evil.test', '/\\evil.test', 'https://evil.test/tasks', '/a\nb', '/login', '/login/callback', '/register?x=1', '/recovery', '/setup']) {
    expect(loginDestination(input)).toBe('/')
  }
})

test('an OAuth consent link keeps only a valid request id', () => {
  const request = 'a'.repeat(43)
  expect(loginDestination(`/oauth/consent?request=${request}&next=https://evil.test`)).toBe(`/oauth/consent?request=${request}`)
  expect(loginDestination('/oauth/consent?request=bad')).toBe('/')
})
