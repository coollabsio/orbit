import { expect, test } from 'bun:test'

import { isMention, notificationCopy, notificationTarget } from './inbox'
import type { NotificationRecord } from './orbit'

const notification = (fields: Partial<NotificationRecord>): NotificationRecord => ({
  id: 'n1',
  kind: 'task_assigned',
  actor_user_id: 'ada',
  recipient_user_id: 'me',
  workspace_id: 'w',
  created_at: '2026-10-23T10:00:00Z',
  ...fields,
})

test('a notification leads to its conversation, page or task', () => {
  expect(notificationTarget(notification({ task_id: 't1' }))).toBe('/tasks/t1')
  expect(notificationTarget(notification({ kind: 'chat_mentioned', chat_conversation_id: 'c1', chat_message_id: 'm1' }))).toBe('/chat/c1')
  expect(notificationTarget(notification({ kind: 'page_mentioned', page_id: 'p1' }))).toBe('/docs/p1')
  expect(notificationTarget(notification({}))).toBe(null)
})

test('copy names the actor and the page like the web', () => {
  expect(notificationCopy(notification({ kind: 'chat_mentioned' }), undefined, 'Ada')).toEqual({ title: 'Ada mentioned you in chat', body: 'Open the conversation to see the message.' })
  expect(notificationCopy(notification({ kind: 'page_mentioned' }), ' ', 'Ada').title).toBe('Ada mentioned you in “Untitled”')
  expect(notificationCopy(notification({ kind: 'page_comment_mentioned' }), 'Roadmap', 'Ada').body).toBe('Someone mentioned you in a comment on “Roadmap”.')
  expect(notificationCopy(notification({ kind: 'comment_mentioned' }), undefined, undefined).body).toBe('Someone mentioned you in a task comment.')
  expect(notificationCopy(notification({}), undefined, 'Ada')).toEqual({ title: 'You were assigned a task', body: 'A task was assigned to you.' })
})

test('mentions are every mention kind, not assignments', () => {
  expect(isMention(notification({ kind: 'page_mentioned' }))).toBe(true)
  expect(isMention(notification({ kind: 'task_assigned' }))).toBe(false)
})
