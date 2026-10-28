import { expect, test } from 'bun:test'
import { guestHome, guestMayOpen } from './guest'

test('a guest opens chat and its account settings only', () => {
  expect(guestMayOpen(guestHome)).toBe(true)
  for (const path of ['/chat', '/chat/c1', '/chat/thread/m1', '/settings', '/settings/profile', '/settings/sessions', '/settings/admin/users']) {
    expect(guestMayOpen(path)).toBe(true)
  }
  for (const path of ['/', '/tasks', '/tasks/t1', '/docs', '/activity', '/settings/members', '/settings/file-storage', '/chatroom', '/settings/profiles']) {
    expect(guestMayOpen(path)).toBe(false)
  }
})
