import { expect, test } from 'bun:test'
import type { AttachmentRecord, AuditEvent, CommentRecord, ProjectRecord, TaskRecord } from '@/api/generated/types.gen'
import { refIdentifier, taskFromRecord, taskIdentifier } from './models'

const project: ProjectRecord = {
  id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'LCH', color: '#123456',
  created_at: '2026-09-04T09:00:00Z', updated_at: '2026-09-04T09:00:00Z', version: 2,
  auto_close_parent: true, auto_close_sub_issues: true,
}
const record: TaskRecord = {
  id: '01HZYTASK000000000000001', workspace_id: 'workspace-1', project_id: project.id, number: 12,
  status_id: 'status-1', title: 'Ship it', description: 'Ready', position: 4, priority: 'high',
  assignee_ids: ['user-1'], creator_id: 'user-1', label_ids: ['label-1'],
  due_at: '2030-01-02T12:30:00.000Z',
  created_at: '2026-09-04T10:00:00Z', updated_at: '2026-09-04T11:00:00Z', duplicate_of: null, blocked: false, version: 7,
  parent: null, parent_task_id: null, sub_issue_count: 0, sub_issue_closed_count: 0,
}
const comment: CommentRecord = {
  id: 'comment-1', workspace_id: 'workspace-1', task_id: record.id, author_id: 'user-1',
  body: 'Looks good', can_edit: true, can_delete: true, created_at: '2026-09-04T12:00:00Z', updated_at: '2026-09-04T12:05:00Z', version: 3,
}
const attachment: AttachmentRecord = {
  id: 'attachment-1', workspace_id: 'workspace-1', task_id: record.id, owner_id: 'user-1',
  display_name: 'brief.pdf', media_type: 'application/pdf', byte_size: 42,
  created_at: '2026-09-04T12:00:00Z',
}
const activity: AuditEvent = {
  id: 'activity-1', workspace_id: 'workspace-1', actor_id: 'user-1', action: 'task.updated',
  outcome: 'success', resource_type: 'task', resource_id: record.id, request_id: 'request-1',
  metadata: {}, occurred_at: '2026-09-04T12:10:00Z',
}

test('generated task records become the existing task view model without mock fallback', () => {
  const task = taskFromRecord(record, project, [comment], [attachment], [activity])

  expect(task).toMatchObject({
    id: record.id,
    identifier: 'LCH-12',
    number: 12,
    projectKey: 'LCH',
    projectId: project.id,
    statusId: 'status-1',
    priority: 'high',
    dueAt: '2030-01-02T12:30:00.000Z',
    comments: [{ id: 'comment-1', body: 'Looks good', version: 3 }],
    attachments: [{ id: 'attachment-1', fileName: 'brief.pdf', fileSize: 42 }],
    activity: [{ id: 'activity-1', actorId: 'user-1', text: 'updated task' }],
    version: 7,
  })
  expect(task.attachments[0]?.url).toBe(
    `/api/v1/workspaces/workspace-1/tasks/${record.id}/attachments/attachment-1/download`,
  )
})

test('service account audit metadata becomes the task activity actor', () => {
  const serviceActivity: AuditEvent = {
    ...activity,
    actor_id: null,
    metadata: { actor_service_account_id: 'service-1', actor_service_account_name: 'Discord' },
  }

  const task = taskFromRecord(record, project, [], [], [serviceActivity])

  expect(task.activity[0]).toMatchObject({
    actorId: '',
    actorName: 'Discord',
    actorServiceAccountId: 'service-1',
  })
})

test('duplicate target and blocked flag reach the task view model', () => {
  const duplicate = taskFromRecord({
    ...record,
    duplicate_of: { id: 'task-91c0', project_id: project.id, project_key: 'LCH', number: 91, title: 'Login fails on Safari' },
    blocked: true,
  }, project)
  expect(duplicate.duplicateOf).toEqual({ id: 'task-91c0', projectId: project.id, number: 91, title: 'Login fails on Safari' })
  expect(duplicate.blocked).toBe(true)
  expect(taskFromRecord({ ...record, duplicate_of: null, blocked: false }, project)).toMatchObject({ duplicateOf: null, blocked: false })
})

test('task identifiers are the project key plus the task number', () => {
  expect(taskIdentifier('01HZYTASK00000000000091c0', project, 12)).toBe('LCH-12')
  expect(refIdentifier({ id: 'task-91c0', projectKey: 'ORB', number: 7 })).toBe('ORB-7')
})

test('a task whose number is not loaded falls back to the last four id characters', () => {
  expect(taskIdentifier('01HZYTASK00000000000091c0', project)).toBe('LCH-91C0')
  expect(taskIdentifier('task-91c0', undefined, null)).toBe('TASK-91C0')
  expect(taskIdentifier('task-91c0', project, 0)).toBe('LCH-91C0')
})

const relationEvent = (action: string, metadata: Record<string, unknown>): AuditEvent => ({ ...activity, id: action, action, metadata })
const other = { related_task_id: 'task-91c0', related_task_title: 'Login fails on Safari', related_task_project_id: project.id }

test('relation audit events read as sentences and link the other task', () => {
  const task = taskFromRecord(record, project, [], [], [
    relationEvent('task.marked_duplicate', { ...other, type: 'duplicate', direction: 'outgoing' }),
    relationEvent('task.unmarked_duplicate', { ...other, type: 'duplicate', direction: 'outgoing' }),
    relationEvent('task.relation_added', { ...other, type: 'blocks', direction: 'incoming' }),
    relationEvent('task.relation_added', { ...other, type: 'blocks', direction: 'outgoing' }),
    relationEvent('task.relation_added', { ...other, type: 'related', direction: 'outgoing' }),
    relationEvent('task.relation_removed', { ...other, type: 'related', direction: 'incoming' }),
    relationEvent('task.relation_removed', { ...other, type: 'blocks', direction: 'incoming' }),
    relationEvent('task.marked_duplicate', { ...other, type: 'duplicate', direction: 'incoming' }),
  ])
  expect(task.activity.map((item) => item.text)).toEqual([
    'marked as duplicate of LCH-91C0 · Login fails on Safari',
    'unmarked as duplicate',
    'added blocker LCH-91C0',
    'blocks LCH-91C0',
    'added related LCH-91C0',
    'removed related LCH-91C0',
    'removed blocker LCH-91C0',
    'marked LCH-91C0 as duplicate',
  ])
  expect(task.activity[0]!.related).toEqual({ taskId: 'task-91c0', identifier: 'LCH-91C0' })
  expect(task.activity[1]!.related).toBeUndefined()
})

test('cross-project relation events use the other project key; missing metadata falls back to plain text', () => {
  const api = { ...project, id: 'project-2', key: 'API' }
  const task = taskFromRecord(record, project, [], [], [
    relationEvent('task.relation_added', { ...other, related_task_project_id: 'project-2', type: 'blocks', direction: 'incoming' }),
    relationEvent('task.relation_added', {}),
    relationEvent('task.updated', {}),
  ], [project, api])
  expect(task.activity.map((item) => item.text)).toEqual(['added blocker API-91C0', 'added relation', 'updated task'])
})

test('activity names a loaded task by its number', () => {
  const task = taskFromRecord(record, project, [], [], [
    relationEvent('task.relation_added', { ...other, type: 'blocks', direction: 'incoming' }),
  ], [project], new Map([['task-91c0', 91]]))
  expect(task.activity.map((item) => item.text)).toEqual(['added blocker LCH-91'])
  expect(task.activity[0]!.related).toEqual({ taskId: 'task-91c0', identifier: 'LCH-91' })
})

test('parent, ancestors and sub-issue counts reach the task view model', () => {
  const task = taskFromRecord({
    ...record,
    parent_task_id: '01HZYPARENT0000000000012',
    parent: { id: '01HZYPARENT0000000000012', title: 'Checkout redesign', project_key: 'ORB', number: 12 },
    sub_issue_count: 5,
    sub_issue_closed_count: 2,
    ancestors: [
      { id: '01HZYROOT00000000000009A', title: 'Q4 launch', project_key: 'ORB', number: 9 },
      { id: '01HZYPARENT0000000000012', title: 'Checkout redesign', project_key: 'ORB', number: 12 },
    ],
  } as TaskRecord, project)
  expect(task).toMatchObject({
    parentTaskId: '01HZYPARENT0000000000012',
    parent: { id: '01HZYPARENT0000000000012', title: 'Checkout redesign', projectKey: 'ORB', number: 12 },
    subIssueCount: 5,
    subIssueClosedCount: 2,
  })
  expect(task.ancestors!.map((ref) => refIdentifier(ref))).toEqual(['ORB-9', 'ORB-12'])
})

test('records without sub-issue fields have no parent, no ancestors and no sub-issues', () => {
  expect(taskFromRecord(record, project)).toMatchObject({ parentTaskId: null, parent: null, subIssueCount: 0, subIssueClosedCount: 0, ancestors: [] })
})

test('parent changes and automatic closes read as sentences that link the other task', () => {
  const event = (action: string, metadata: Record<string, unknown>): AuditEvent => ({ ...activity, id: action, action, metadata })
  const task = taskFromRecord({
    ...record,
    parent_task_id: '01HZYPARENT0000000000012',
    parent: { id: '01HZYPARENT0000000000012', title: 'Checkout redesign', project_key: 'ORB', number: 12 },
  } as TaskRecord, project, [], [], [
    event('task.parent_changed', { from: null, to: '01HZYPARENT0000000000012' }),
    event('task.parent_changed', { from: '01HZYOTHER00000000000099', to: null }),
    event('task.auto_closed', { source_task_id: '01HZYPARENT0000000000012', from_status_id: 'status-1', to_status_id: 'status-done', reason: 'parent_closed' }),
    event('task.auto_closed', { source_task_id: '01HZYCHILD00000000000031', from_status_id: 'status-1', to_status_id: 'status-done', reason: 'sub_issues_done' }),
  ])
  expect(task.activity.map((item) => item.text)).toEqual([
    'set parent to ORB-12',
    'removed parent LCH-0099',
    'closed automatically because ORB-12 was closed',
    'closed automatically because all sub-issues were done',
  ])
  expect(task.activity[0]!.related).toEqual({ taskId: '01HZYPARENT0000000000012', identifier: 'ORB-12' })
  expect(task.activity[2]!.statusId).toBe('status-done')
})

test('an update event becomes one row per changed field; an event with nothing to show is dropped', () => {
  const task = taskFromRecord(record, project, [], [], [
    relationEvent('task.updated', { changes: { status: { from: 'todo', to: 'done' }, labels: { added: ['label-1'], removed: [] }, description: true } }),
    relationEvent('task.updated', { changes: {} }),
  ], [project])
  expect(task.activity.map((item) => item.change)).toEqual([
    { field: 'status', from: 'todo', to: 'done' },
    { field: 'labels', added: ['label-1'], removed: [] },
    { field: 'description' },
  ])
  expect(task.activity[0]!.statusId).toBe('done')
})
