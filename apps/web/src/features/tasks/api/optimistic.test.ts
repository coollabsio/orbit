import { expect, test } from 'bun:test'
import { QueryClient } from '@tanstack/react-query'
import { queryKeys } from '../../../api/queryKeys'
import type { PageTaskRecord, TaskRecord } from '../../../api/generated/types.gen'
import { patchWorkspaceTask, reconcileWorkspaceTask, restoreWorkspaceTasks } from './optimistic'

const task = (workspaceId: string, version = 1): TaskRecord => ({
  id: 'task-1', workspace_id: workspaceId, project_id: 'project-1', status_id: 'todo', identifier: 'GEN-1', identifier_key: 'GEN', number: 1, title: 'Before',
  description_json: { type: 'doc', content: [] }, description_text: '', position: 1, priority: 'none', assignee_ids: [], creator_id: 'user-1', label_ids: [],
  sub_issue_total: 0, sub_issue_done: 0, duplicate_ids: [],
  created_at: '2026-09-04T10:00:00Z', updated_at: '2026-09-04T10:00:00Z', version,
})

test('failed optimistic updates restore every captured cache entry in only the affected workspace', () => {
  const client = new QueryClient()
  const aKey = queryKeys.tasks.list('workspace-a')
  const bKey = queryKeys.tasks.list('workspace-b')
  const detailKey = queryKeys.tasks.detail('workspace-a', 'task-1')
  const aPage: PageTaskRecord = { items: [task('workspace-a')] }
  const bPage: PageTaskRecord = { items: [task('workspace-b')] }
  client.setQueryData(aKey, aPage)
  client.setQueryData(detailKey, task('workspace-a'))
  client.setQueryData(bKey, bPage)

  const snapshot = patchWorkspaceTask(client, 'workspace-a', 'task-1', { title: 'Optimistic' })
  expect((client.getQueryData<PageTaskRecord>(aKey)?.items[0])?.title).toBe('Optimistic')
  expect(client.getQueryData<TaskRecord>(detailKey)?.title).toBe('Optimistic')
  expect((client.getQueryData<PageTaskRecord>(bKey)?.items[0])?.title).toBe('Before')

  restoreWorkspaceTasks(client, snapshot)
  expect(client.getQueryData<PageTaskRecord>(aKey)).toEqual(aPage)
  expect(client.getQueryData<TaskRecord>(detailKey)).toEqual(task('workspace-a'))
})

test('successful optimistic updates reconcile the authoritative server version', () => {
  const client = new QueryClient()
  const key = queryKeys.tasks.list('workspace-a')
  client.setQueryData<PageTaskRecord>(key, { items: [task('workspace-a')] })

  reconcileWorkspaceTask(client, 'workspace-a', { ...task('workspace-a', 2), title: 'Server title' })

  expect(client.getQueryData<PageTaskRecord>(key)?.items[0]).toMatchObject({ title: 'Server title', version: 2 })
})
