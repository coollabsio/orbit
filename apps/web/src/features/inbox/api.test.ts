import { expect, test } from 'bun:test'
import type { NotificationRecord } from '@/api/generated/types.gen'
import { isMention, notificationCopy, notificationTarget, snoozePresets } from './api'

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

test('a task row shows the task and its latest event', () => {
  const task = { ...base, task_id: 't1', task_identifier: 'ENG-12', task_title: 'Ship the parser' }
  const body = (kind: string, actor?: string, actorId: string | null = 'u2') =>
    notificationCopy({ ...task, kind, actor_user_id: actorId }, undefined, actor).body
  expect(notificationCopy({ ...task, kind: 'task_assigned' }, undefined, 'Ada').title).toBe('ENG-12 Ship the parser')
  expect(body('task_assigned', 'Ada')).toBe('Ada · assigned you')
  expect(body('comment_mentioned', 'Ada')).toBe('Ada · mentioned you in a comment')
  expect(body('task_commented', 'Ada')).toBe('Ada · commented')
  expect(body('task_status_changed', 'Ada')).toBe('Ada · changed the status')
  expect(body('task_blocked', 'Ada')).toBe('Ada · the task is now blocked')
  expect(body('task_unblocked', 'Ada')).toBe('Ada · the task is no longer blocked')
  // GitHub has no user; a member who left has an id and no name.
  expect(body('task_status_changed', undefined, null)).toBe('GitHub · changed the status')
  expect(body('task_status_changed')).toBe('Someone · changed the status')
  // A kind that a newer server added, and a task without a title.
  expect(body('task_due_soon', 'Ada')).toBe('Ada · updated the task')
  expect(notificationCopy({ ...task, kind: 'task_assigned', task_title: ' ', task_identifier: null }).title).toBe('Untitled')
  expect(isMention({ ...task, kind: 'task_commented' })).toBe(false)
})

test('page and chat rows keep their text', () => {
  expect(notificationCopy({ ...base, kind: 'page_mentioned', page_id: 'p1' }, 'Roadmap', 'Ada'))
    .toEqual({ title: 'Ada mentioned you in “Roadmap”', body: 'Ada · Open the page to see where.' })
  expect(notificationCopy({ ...base, kind: 'page_comment_mentioned', page_id: 'p1' }, 'Roadmap').body)
    .toBe('Someone mentioned you in a comment on “Roadmap”.')
})

test('snooze presets are an hour ahead, tomorrow morning and next Monday morning', () => {
  const at = (now: Date) => snoozePresets(now).map((preset) => `${preset.label}: ${preset.until.toString().slice(0, 21)}`)
  // a Thursday afternoon
  expect(at(new Date(2026, 9, 8, 15, 30))).toEqual([
    'In 1 hour: Thu Oct 08 2026 16:30',
    'Tomorrow: Fri Oct 09 2026 09:00',
    'Next week: Mon Oct 12 2026 09:00',
  ])
  // on a Monday "next week" is the Monday after, and on a Sunday it is not "tomorrow" twice over
  expect(at(new Date(2026, 9, 12, 8))[2]).toBe('Next week: Mon Oct 19 2026 09:00')
  expect(at(new Date(2026, 9, 11, 23, 59))[2]).toBe('Next week: Mon Oct 12 2026 09:00')
  // every preset is in the future, also across a month end
  const late = new Date(2026, 9, 31, 23, 59)
  expect(snoozePresets(late).every((preset) => preset.until > late)).toBe(true)
})
