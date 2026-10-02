import { expect, test } from 'bun:test'
import type { NotificationRecord } from '@/api/generated/types.gen'
import { isMention, notificationTarget } from './api'

const base: NotificationRecord = {
  id: 'n1',
  workspace_id: 'w1',
  recipient_user_id: 'u1',
  actor_user_id: 'u2',
  kind: 'chat_mentioned',
  created_at: '2026-10-02T08:00:00Z',
}

test('a chat mention opens the conversation at the message', () => {
  const notification = { ...base, chat_conversation_id: 'c1', chat_message_id: 'm 1' }
  expect(notificationTarget(notification)).toBe('/chat/c1?m=m%201')
  expect(isMention(notification)).toBe(true)
  // Without its conversation there is nowhere to go.
  expect(notificationTarget(base)).toBeNull()
})
