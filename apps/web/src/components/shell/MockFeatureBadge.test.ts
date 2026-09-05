import { expect, test } from 'bun:test'
import { isMockBackedPath } from './mockFeatures'

test('mock badges distinguish persistent milestone routes from mock-backed features', () => {
  for (const path of ['/', '/docs', '/docs/one', '/mail', '/chat/general', '/dm/one', '/inbox', '/profile']) {
    expect(isMockBackedPath(path)).toBeTrue()
  }
  for (const path of ['/tasks', '/tasks/one', '/tasks-trash', '/settings', '/settings/members', '/settings/sessions']) {
    expect(isMockBackedPath(path)).toBeFalse()
  }
})
