import { expect, test } from 'bun:test'
import { isMockBackedPath } from './mockFeatures'

test('mock badges distinguish persistent milestone routes from mock-backed features', () => {
  for (const path of ['/', '/mail', '/inbox']) {
    expect(isMockBackedPath(path)).toBeTrue()
  }
  for (const path of ['/tasks', '/tasks/one', '/tasks-trash', '/docs', '/docs/one', '/docs/trash', '/settings', '/settings/members', '/settings/sessions', '/profile', '/dm/one', '/chat', '/chat/general']) {
    expect(isMockBackedPath(path)).toBeFalse()
  }
})
