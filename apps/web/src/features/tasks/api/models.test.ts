import { expect, test } from 'bun:test'
import type { AttachmentRecord, AuditEvent, CommentRecord, ProjectRecord, TaskRecord } from '../../../api/generated/types.gen'
import { EMPTY_DOCUMENT } from '../../../components/editor/document'
import { taskFromRecord } from './models'

const project: ProjectRecord = {
  id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'LCH', color: '#123456',
  created_at: '2026-09-04T09:00:00Z', updated_at: '2026-09-04T09:00:00Z', version: 2,
}
const record: TaskRecord = {
  id: '01HZYTASK000000000000001', workspace_id: 'workspace-1', project_id: project.id,
  status_id: 'status-1', identifier: 'LCH-12', identifier_key: 'LCH', number: 12, title: 'Ship it', description_json: { type: 'doc', content: [] }, description_text: 'Ready', position: 4, priority: 'high',
  assignee_ids: ['user-1'], creator_id: 'user-1', label_ids: ['label-1'],
  due_at: '2030-01-02T12:30:00.000Z',
  sub_issue_total: 0, sub_issue_done: 0, duplicate_ids: [], referenced_by: [],
  created_at: '2026-09-04T10:00:00Z', updated_at: '2026-09-04T11:00:00Z', version: 7,
}
const comment: CommentRecord = {
  id: 'comment-1', workspace_id: 'workspace-1', task_id: record.id, author_id: 'user-1',
  body_json: { type: 'doc', content: [] }, body_text: 'Looks good', created_at: '2026-09-04T12:00:00Z', updated_at: '2026-09-04T12:05:00Z', version: 3,
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
  const task = taskFromRecord(record, [comment], [attachment], [activity])

  expect(task).toMatchObject({
    id: record.id,
    identifier: 'LCH-12',
    projectId: project.id,
    statusId: 'status-1',
    priority: 'high',
    dueAt: '2030-01-02T12:30:00.000Z',
    comments: [{ id: 'comment-1', bodyJson: { type: 'doc', content: [] }, bodyText: 'Looks good', version: 3 }],
    attachments: [{ id: 'attachment-1', fileName: 'brief.pdf', fileSize: 42 }],
    activity: [{ id: 'activity-1', actorId: 'user-1', text: 'Updated task' }],
    version: 7,
  })
  expect(task.attachments[0]?.url).toBe(
    `/api/v1/workspaces/workspace-1/tasks/${record.id}/attachments/attachment-1/download`,
  )
})

const shipIt = {
  type: 'doc',
  content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Ship it' }] }],
}

test('a task carries the stored document and the server-derived text', () => {
  const task = taskFromRecord({ ...record, description_json: shipIt, description_text: 'Ship it' })

  expect(task.descriptionJson).toEqual(shipIt as never)
  expect(task.descriptionText).toBe('Ship it')
  expect('description' in task).toBe(false)
})

test('a malformed document degrades to an empty document instead of throwing', () => {
  const task = taskFromRecord({ ...record, description_json: 'nope' as never })

  expect(task.descriptionJson).toEqual(EMPTY_DOCUMENT)
})

test('a comment carries its document and derived text', () => {
  const task = taskFromRecord(record, [{ ...comment, body_json: shipIt, body_text: 'Ship it' }])

  expect(task.comments[0].bodyJson).toEqual(shipIt as never)
  expect(task.comments[0].bodyText).toBe('Ship it')
  expect('body' in task.comments[0]).toBe(false)
})

test('taskFromRecord carries the task graph fields', () => {
  const graph: TaskRecord = {
    ...record,
    parent_id: 'task-0', sub_issue_total: 5, sub_issue_done: 3,
    duplicate_of_task_id: 'task-9', duplicate_ids: ['task-7'],
    referenced_by: [{
      source_type: 'comment', source_id: 'c-1', source_task_id: 'task-4',
      source_task_identifier: 'LCH-4', source_task_title: 'Plan',
    }],
  }

  const task = taskFromRecord(graph)

  expect(task.parentId).toBe('task-0')
  expect(task.subIssueTotal).toBe(5)
  expect(task.subIssueDone).toBe(3)
  expect(task.duplicateOfTaskId).toBe('task-9')
  expect(task.duplicateIds).toEqual(['task-7'])
  expect(task.referencedBy).toEqual([{
    sourceType: 'comment', sourceId: 'c-1', sourceTaskId: 'task-4',
    sourceTaskIdentifier: 'LCH-4', sourceTaskTitle: 'Plan',
  }])
})

test('taskFromRecord defaults the graph fields for a plain task', () => {
  const task = taskFromRecord(record)
  expect(task.parentId).toBeNull()
  expect(task.duplicateOfTaskId).toBeNull()
  expect(task.duplicateIds).toEqual([])
  expect(task.referencedBy).toEqual([])
})

test('service account audit metadata becomes the task activity actor', () => {
  const serviceActivity: AuditEvent = {
    ...activity,
    actor_id: null,
    metadata: { actor_service_account_id: 'service-1', actor_service_account_name: 'Discord' },
  }

  const task = taskFromRecord(record, [], [], [serviceActivity])

  expect(task.activity[0]).toMatchObject({
    actorId: '',
    actorName: 'Discord',
    actorServiceAccountId: 'service-1',
  })
})
