import { expect, test } from 'bun:test'
import { coreSettingsPaths, disabledProductPaths, isDisabledProductPath, mobileDockPaths, primaryProductPath } from './productNavigation'

test('tasks are the landing page, docs are enabled and mock product routes are disabled', () => {
  expect(primaryProductPath).toBe('/tasks')
  expect(disabledProductPaths).toEqual(['/mail', '/chat', '/dm'])
  expect(coreSettingsPaths).toEqual(['/settings', '/settings/members', '/settings/sessions', '/settings/danger-zone', '/tasks-trash'])
  expect(mobileDockPaths).toEqual(['/tasks', '/docs', '/settings'])

  for (const path of ['/mail/thread', '/chat/general', '/dm/one']) {
    expect(isDisabledProductPath(path)).toBeTrue()
  }
  for (const path of ['/tasks', '/tasks/one', '/tasks-trash', '/docs', '/docs/one', '/docs/trash', '/settings', '/profile', '/inbox']) {
    expect(isDisabledProductPath(path)).toBeFalse()
  }
})
