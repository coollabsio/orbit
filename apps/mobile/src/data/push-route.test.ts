import { expect, test } from 'bun:test'

import { pushRoute, pushUrl } from './push-route'

test('maps the paths the server sends to app routes', () => {
  expect(pushRoute('/chat/c1')).toBe('/chat/c1')
  expect(pushRoute('/chat/c1?thread=m9')).toBe('/chat/thread/m9')
  expect(pushRoute('/chat/c1/thread/m9')).toBe('/chat/thread/m9')
  expect(pushRoute('/chat/unreads')).toBe('/chat/unreads')
  expect(pushRoute('/activity')).toBe('/activity')
  expect(pushRoute('/inbox')).toBe('/activity')
  expect(pushRoute('/profile/notifications')).toBe('/settings/notifications')
  expect(pushRoute('/settings/notifications')).toBe('/settings/notifications')
})

test('maps tasks, views and docs', () => {
  expect(pushRoute('/tasks/t1')).toBe('/tasks/t1')
  expect(pushRoute('/tasks/projects/p1/settings')).toBe('/tasks')
  expect(pushRoute('/views/v1/t1')).toBe('/tasks/t1')
  expect(pushRoute('/docs/p1')).toBe('/docs/p1')
  expect(pushRoute('/docs/trash')).toBe('/docs/trash')
  expect(pushRoute('/docs')).toBe('/docs')
})

test('accepts a full URL, the root, and refuses what the app has no screen for', () => {
  expect(pushRoute('https://app.orbit.so/chat/c1#x')).toBe('/chat/c1')
  expect(pushRoute('/')).toBe('/')
  expect(pushRoute('/admin/users')).toBeNull()
})

test('reads the url from the iOS remote payload, then from data', () => {
  const remote = { content: { data: null }, trigger: { type: 'push', payload: { aps: {}, url: '/chat/c1' } } }
  expect(pushUrl(remote)).toBe('/chat/c1')
  expect(pushUrl({ content: { data: { url: '/activity' } }, trigger: null })).toBe('/activity')
  expect(pushUrl({ content: { data: {} }, trigger: { type: 'push', payload: { aps: {} } } })).toBeNull()
})

test('pull request paths open the pull request screens', () => {
  expect(pushRoute('https://orbit.test/tasks/pulls/acme/repo/12')).toBe('/tasks/pulls/acme/repo/12')
  expect(pushRoute('/tasks/pulls/acme/repo/12?comment=5')).toBe('/tasks/pulls/acme/repo/12?comment=5')
  expect(pushRoute('/tasks/projects/p1/pulls')).toBe('/tasks/project/p1/pulls')
  expect(pushRoute('/tasks/projects/p1/settings')).toBe('/tasks')
  expect(pushRoute('/tasks/pulls/acme')).toBe('/tasks')
})
