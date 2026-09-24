import { expect, test } from 'bun:test'
import type { AttachmentRecord, AuditEvent, CommentRecord, ProjectRecord, TaskRecord } from '@/api/generated/types.gen'
import { taskFromRecord, taskIdentifier } from './models'

const project: ProjectRecord = {
  id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'LCH', color: '#123456',
  created_at: '2026-09-04T09:00:00Z', updated_at: '2026-09-04T09:00:00Z', version: 2,
}
const record: TaskRecord = {
  id: '01HZYTASK000000000000001', workspace_id: 'workspace-1', project_id: project.id,
  status_id: 'status-1', title: 'Ship it', description: 'Ready', position: 4, priority: 'high',
  assignee_ids: ['user-1'], creator_id: 'user-1', label_ids: ['label-1'],
  due_at: '2030-01-02T12:30:00.000Z',
  created_at: '2026-09-04T10:00:00Z', updated_at: '2026-09-04T11:00:00Z', duplicate_of: null, blocked: false, version: 7,
}
const comment: CommentRecord = {
  id: 'comment-1', workspace_id: 'workspace-1', task_id: record.id, author_id: 'user-1',
  body: 'Looks good', created_at: '2026-09-04T12:00:00Z', updated_at: '2026-09-04T12:05:00Z', version: 3,
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
    identifier: 'LCH-0001',
    projectId: project.id,
    statusId: 'status-1',
    priority: 'high',
    dueAt: '2030-01-02T12:30:00.000Z',
    comments: [{ id: 'comment-1', body: 'Looks good', version: 3 }],
    attachments: [{ id: 'attachment-1', fileName: 'brief.pdf', fileSize: 42 }],
    activity: [{ id: 'activity-1', actorId: 'user-1', text: 'Updated task' }],
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
    duplicate_of: { id: 'task-91c0', project_id: project.id, title: 'Login fails on Safari' },
    blocked: true,
  }, project)
  expect(duplicate.duplicateOf).toEqual({ id: 'task-91c0', projectId: project.id, title: 'Login fails on Safari' })
  expect(duplicate.blocked).toBe(true)
  expect(taskFromRecord({ ...record, duplicate_of: null, blocked: false }, project)).toMatchObject({ duplicateOf: null, blocked: false })
})

test('task identifiers are the project key plus the last four id characters', () => {
  expect(taskIdentifier('01HZYTASK00000000000091c0', project)).toBe('LCH-91C0')
  expect(taskIdentifier('task-91c0', undefined)).toBe('TASK-91C0')
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
    'Marked as duplicate of LCH-91C0 · Login fails on Safari',
    'Unmarked as duplicate',
    'Added blocker LCH-91C0',
    'Blocks LCH-91C0',
    'Added related LCH-91C0',
    'Removed related LCH-91C0',
    'Removed blocker LCH-91C0',
    'Marked LCH-91C0 as duplicate',
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
  expect(task.activity.map((item) => item.text)).toEqual(['Added blocker API-91C0', 'Added relation', 'Updated task'])
})
