import { describe, expect, test } from 'bun:test'

import { activityRows, buildFeed, foldRun, type ActivityContext, type ActivityRow } from './activity'
import type { CommentRecord, TaskActivityEvent } from './orbit'

const context: ActivityContext = {
  statuses: [
    { id: 's-backlog', name: 'Backlog' },
    { id: 's-done', name: 'Done' },
  ],
  people: [{ id: 'ada', name: 'Ada Lovelace' }],
  labels: [{ id: 'l-bug', name: 'Bug' }],
  projects: [
    { id: 'p1', name: 'Foundation' },
    { id: 'p2', name: 'Mobile' },
  ],
  identifierOf: (taskId) => (taskId === 't2' ? 'GEN-2' : 'a task'),
  locale: 'en-US',
}

const event = (id: string, action: string, metadata: unknown, at = '2026-10-15T10:00:00Z'): TaskActivityEvent => ({
  id,
  action,
  actor_id: 'ada',
  metadata,
  occurred_at: at,
  outcome: 'success',
  request_id: 'r',
  resource_type: 'task',
})

const comment = (id: string, at: string, parent_id: string | null = null): CommentRecord => ({
  id,
  author_id: 'ada',
  body: id,
  body_json: null,
  can_delete: true,
  can_edit: true,
  created_at: at,
  updated_at: at,
  parent_id,
  task_id: 't1',
  version: 1,
  workspace_id: 'w',
})

describe('activityRows', () => {
  test('a task.updated event gives one sentence per changed field', () => {
    const rows = activityRows(
      [
        event('e1', 'task.updated', {
          changes: {
            status: { from: 's-backlog', to: 's-done' },
            priority: { from: 'none', to: 'high' },
            assignees: { added: ['ada'], removed: ['gone'] },
            labels: { added: ['l-bug'], removed: [] },
            project: { from: 'p1', to: 'p2' },
            due: { start: null, end: '2026-10-30T12:00:00Z' },
            description: true,
          },
        }),
      ],
      context,
    )
    expect(rows.map((row) => [row.id, row.text, row.statusId])).toEqual([
      ['e1:status', 'changed status from Backlog to Done', 's-done'],
      ['e1:priority', 'set priority to High', undefined],
      ['e1:assignees', 'assigned Ada Lovelace and unassigned someone', undefined],
      ['e1:labels', 'added label Bug', undefined],
      ['e1:project', 'moved from Foundation to Mobile', undefined],
      ['e1:due', 'set due date to Oct 30', undefined],
      ['e1:description', 'updated the description', undefined],
    ])
  })

  test('relations, parents, automation and the generic fallback', () => {
    const rows = activityRows(
      [
        event('e1', 'task.relation_added', { related_task_id: 't2', type: 'blocks', direction: 'incoming' }),
        event('e2', 'task.parent_changed', { to: 't2' }),
        event('e3', 'task.auto_closed', { to_status_id: 's-done' }),
        event('e4', 'task.created', {}),
        event('e5', 'task.updated', null),
      ],
      context,
    )
    expect(rows.map((row) => [row.text, row.statusId])).toEqual([
      ['added blocker GEN-2', undefined],
      ['set parent to GEN-2', undefined],
      ['closed automatically because all sub-issues were done', 's-done'],
      ['created task', undefined],
      ['updated task', undefined],
    ])
  })
})

describe('buildFeed', () => {
  test('interleaves rows and threads by time, joining consecutive rows', () => {
    const row = (id: string, createdAt: string): ActivityRow => ({ id, actorId: 'ada', text: id, createdAt })
    const feed = buildFeed(
      [row('a', '2026-10-15T09:00:00Z'), row('b', '2026-10-15T09:30:00Z'), row('c', '2026-10-15T11:00:00Z')],
      [comment('reply', '2026-10-15T10:30:00Z', 'root'), comment('root', '2026-10-15T10:00:00Z')],
    )
    expect(feed.map((entry) => (entry.kind === 'activity' ? entry.items.map((item) => item.id) : [`${entry.thread.root.id}+${entry.thread.replies.map((reply) => reply.id)}`]))).toEqual([
      ['a', 'b'],
      ['root+reply'],
      ['c'],
    ])
  })
})

describe('foldRun', () => {
  test('keeps the first and the five latest rows of a long run', () => {
    const run = Array.from({ length: 8 }, (_, index): ActivityRow => ({ id: `r${index}`, actorId: 'ada', text: '', createdAt: '' }))
    const folded = foldRun(run)
    expect(folded.hidden).toBe(3)
    expect(folded.items.map((item) => item.id)).toEqual(['r0', 'r4', 'r5', 'r6', 'r7'])
    expect(foldRun(run.slice(0, 6)).hidden).toBe(0)
  })
})
