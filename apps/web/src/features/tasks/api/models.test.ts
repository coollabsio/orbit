import { expect, test } from 'bun:test'
import type { AttachmentRecord, AuditEvent, CommentRecord, ProjectRecord, TaskRecord } from '../../../api/generated/types.gen'
import { taskFromRecord } from './models'

const project: ProjectRecord = {
  id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'LCH', color: '#123456',
  created_at: '2026-09-04T09:00:00Z', updated_at: '2026-09-04T09:00:00Z', version: 2,
}
const record: TaskRecord = {
  id: '01HZYTASK000000000000001', workspace_id: 'workspace-1', project_id: project.id,
  status_id: 'status-1', title: 'Ship it', description: 'Ready', position: 4, priority: 'high',
  assignee_ids: ['user-1'], creator_id: 'user-1', label_ids: ['label-1'],
  due_at: '2030-01-02T12:30:00.000Z',
  created_at: '2026-09-04T10:00:00Z', updated_at: '2026-09-04T11:00:00Z', version: 7,
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
