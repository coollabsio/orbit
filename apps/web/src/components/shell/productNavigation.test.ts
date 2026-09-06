import { expect, test } from 'bun:test'
import { coreSettingsPaths, disabledProductPaths, isDisabledProductPath, mobileDockPaths, primaryProductPath } from './productNavigation'

test('tasks are the landing page and mock product routes are disabled', () => {
  expect(primaryProductPath).toBe('/tasks')
  expect(disabledProductPaths).toEqual(['/docs', '/mail', '/chat', '/dm'])
  expect(coreSettingsPaths).toEqual(['/settings', '/settings/members', '/settings/sessions', '/settings/danger-zone', '/tasks-trash'])
  expect(mobileDockPaths).toEqual(['/tasks', '/settings'])

  for (const path of ['/docs', '/docs/one', '/mail/thread', '/chat/general', '/dm/one']) {
    expect(isDisabledProductPath(path)).toBeTrue()
  }
  for (const path of ['/tasks', '/tasks/one', '/tasks-trash', '/settings', '/profile', '/inbox']) {
    expect(isDisabledProductPath(path)).toBeFalse()
  }
})
