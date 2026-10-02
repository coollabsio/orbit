import { expect, test } from 'bun:test'
import {
  chatEnabled,
  coreSettingsPaths,
  disabledProductPaths,
  disabledProductPathsFor,
  isChatConversationPath,
  isDisabledProductPath,
  mobileDockPaths,
  mobileDockPathsFor,
  primaryProductPath,
} from './productNavigation'

test('tasks are the landing page, docs are enabled and mock product routes are disabled', () => {
  expect(primaryProductPath).toBe('/tasks')
  expect(coreSettingsPaths).toEqual(['/settings', '/settings/members', '/settings/sessions', '/settings/danger-zone', '/tasks-trash'])

  expect(isDisabledProductPath('/mail/thread')).toBeTrue()
  for (const path of ['/tasks', '/tasks/one', '/tasks-trash', '/docs', '/docs/one', '/docs/trash', '/settings', '/profile', '/inbox']) {
    expect(isDisabledProductPath(path)).toBeFalse()
  }
})

test('chat is a product only while it is turned on, and direct messages are not a product of their own', () => {
  expect(disabledProductPathsFor(false)).toEqual(['/mail', '/chat'])
  expect(mobileDockPathsFor(false)).toEqual(['/tasks', '/docs', '/settings'])
  expect(disabledProductPathsFor(true)).toEqual(['/mail'])
  expect(mobileDockPathsFor(true)).toEqual(['/tasks', '/docs', '/chat', '/settings'])

  // this build follows its own setting
  expect(disabledProductPaths).toEqual(disabledProductPathsFor(chatEnabled))
  expect(mobileDockPaths).toEqual(mobileDockPathsFor(chatEnabled))
  expect(isDisabledProductPath('/chat/general')).toBe(!chatEnabled)
})

test('chat is on in every build', () => {
  expect(chatEnabled).toBeTrue()
})

test('everything below /chat is a full screen on a phone', () => {
  for (const path of ['/chat/c1', '/chat/c1/thread/m1', '/chat/unreads', '/chat/threads']) {
    expect(isChatConversationPath(path)).toBeTrue()
  }
  for (const path of ['/chat', '/chat/', '/chats/c1', '/tasks/chat/c1', '/']) {
    expect(isChatConversationPath(path)).toBeFalse()
  }
})
