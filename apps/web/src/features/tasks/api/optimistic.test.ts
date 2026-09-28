import { expect, test } from 'bun:test'
import { QueryClient } from '@tanstack/react-query'
import { queryKeys } from '@/api/queryKeys'
import type { PageTaskRecord, TaskRecord } from '@/api/generated/types.gen'
import { cachedDescendantIds, findCachedTask, patchWorkspaceTask, reconcileWorkspaceTask, restoreWorkspaceTasks } from './optimistic'

const task = (workspaceId: string, version = 1): TaskRecord => ({
  id: 'task-1', workspace_id: workspaceId, project_id: 'project-1', status_id: 'todo', title: 'Before',
  description: '', position: 1, priority: 'none', assignee_ids: [], creator_id: 'user-1', label_ids: [],
  created_at: '2026-09-04T10:00:00Z', updated_at: '2026-09-04T10:00:00Z', duplicate_of: null, blocked: false, version,
  parent: null, parent_task_id: null, sub_issue_count: 0, sub_issue_closed_count: 0,
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

test('a write response keeps the detail breadcrumb, or narrows it to the new parent until the refetch', () => {
  const client = new QueryClient()
  const detailKey = queryKeys.tasks.detail('workspace-a', 'task-1')
  const root = { id: 'task-0009', project_id: 'project-1', project_key: 'ORB', title: 'Root' }
  const parent = { id: 'task-0012', project_id: 'project-1', project_key: 'ORB', title: 'Parent' }
  client.setQueryData(detailKey, { ...task('workspace-a'), parent_task_id: 'task-0012', ancestors: [root, parent] })

  reconcileWorkspaceTask(client, 'workspace-a', { ...task('workspace-a', 2), parent_task_id: 'task-0012' })
  expect(client.getQueryData<TaskRecord>(detailKey)?.ancestors).toEqual([root, parent])

  const moved = { id: 'task-0040', project_id: 'project-1', title: 'Other', project_key: 'ORB' }
  reconcileWorkspaceTask(client, 'workspace-a', { ...task('workspace-a', 3), parent_task_id: 'task-0040', parent: moved })
  expect(client.getQueryData<TaskRecord>(detailKey)?.ancestors?.map((ref) => ref.id)).toEqual(['task-0040'])

  reconcileWorkspaceTask(client, 'workspace-a', task('workspace-a', 4))
  expect(client.getQueryData<TaskRecord>(detailKey)?.ancestors).toEqual([])
})

test('a cached task is found in any list, page or detail query of its workspace', () => {
  const client = new QueryClient()
  client.setQueryData(queryKeys.tasks.list('workspace-a', {}), { pages: [{ items: [task('workspace-a')], next_cursor: null }], pageParams: [undefined] })
  expect(findCachedTask(client, 'workspace-a', 'task-1')?.title).toBe('Before')
  expect(findCachedTask(client, 'workspace-a', 'task-9')).toBeUndefined()
  expect(findCachedTask(client, 'workspace-b', 'task-1')).toBeUndefined()
})

test('descendants come from every cached task query, at any depth', () => {
  const client = new QueryClient()
  const at = (id: string, parentId: string | null) => ({ ...task('workspace-a'), id, parent_task_id: parentId })
  client.setQueryData(queryKeys.tasks.list('workspace-a', {}), { pages: [{ items: [at('task-1', null), at('task-2', 'task-1'), at('task-9', null)] }], pageParams: [undefined] })
  // a nested level the detail loaded, and a sibling list that knows a deeper one
  client.setQueryData([...queryKeys.tasks.all('workspace-a'), 'sub-issues', 'task-2'], { items: [at('task-3', 'task-2')], next_cursor: null })
  client.setQueryData(queryKeys.tasks.list('workspace-a', { q: 'x' }), { items: [at('task-4', 'task-3')] })
  expect(cachedDescendantIds(client, 'workspace-a', 'task-1').sort()).toEqual(['task-2', 'task-3', 'task-4'])
  expect(cachedDescendantIds(client, 'workspace-a', 'task-9')).toEqual([])
})
