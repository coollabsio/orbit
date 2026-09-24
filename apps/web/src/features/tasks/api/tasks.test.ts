import { expect, test } from 'bun:test'
import { createApiClient } from '@/api/client'
import { bulkSetTaskDuplicateOf, markDuplicateUpdates, nextTaskCursor, optimisticTaskPatch, setTaskDuplicateOf, taskListAllPages, taskListPage } from './tasks'

test('task filters and cursor continuation are sent through the generated list call', async () => {
  let requested = ''
  const client = createApiClient({
    fetch: async (request) => {
      requested = request.url
      return Response.json({ items: [], next_cursor: 'cursor-2' })
    },
  })

  const page = await taskListPage(client, 'workspace-1', { project_id: 'project-1', priority: 'high', limit: 25 }, 'cursor-1')
  const url = new URL(requested)

  expect(url.pathname).toBe('/api/v1/workspaces/workspace-1/tasks')
  expect(url.searchParams.get('project_id')).toBe('project-1')
  expect(url.searchParams.get('priority')).toBe('high')
  expect(url.searchParams.get('cursor')).toBe('cursor-1')
  expect(nextTaskCursor(page)).toBe('cursor-2')
})

test('exhaustive task filtering follows every server cursor', async () => {
  const cursors: Array<string | null> = []
  const client = createApiClient({
    fetch: async (request) => {
      const cursor = new URL(request.url).searchParams.get('cursor')
      cursors.push(cursor)
      return Response.json(cursor
        ? { items: [{ id: 'task-2' }], next_cursor: null }
        : { items: [{ id: 'task-1' }], next_cursor: 'cursor-2' })
    },
  })

  const page = await taskListAllPages(client, 'workspace-1', { sort: 'position', order: 'asc', limit: 50 })

  expect(cursors).toEqual([null, 'cursor-2'])
  expect(page.items.map((item) => item.id)).toEqual(['task-1', 'task-2'])
  expect(page.next_cursor).toBeNull()
})

test('duplicate marking is one patch field; bulk items carry it per task', () => {
  expect(markDuplicateUpdates([{ id: 'a', version: 1 }, { id: 'b', version: 7 }], 'target')).toEqual([
    { id: 'a', expected_version: 1, duplicate_of_id: 'target' },
    { id: 'b', expected_version: 7, duplicate_of_id: 'target' },
  ])
  expect(markDuplicateUpdates([{ id: 'a', version: 2 }], null)).toEqual([{ id: 'a', expected_version: 2, duplicate_of_id: null }])
})

test('the optimistic patch never writes duplicate_of_id into cached task records', () => {
  expect(optimisticTaskPatch({ duplicate_of_id: 'target', priority: 'high' })).toEqual({ priority: 'high' })
})

test('duplicate calls go through the task PATCH and the atomic bulk endpoint', async () => {
  const requests: Array<{ method: string; path: string; body: unknown }> = []
  const client = createApiClient({
    fetch: async (request) => {
      requests.push({ method: request.method, path: new URL(request.url).pathname, body: await request.json() })
      return Response.json(request.url.endsWith('/bulk') ? { items: [], next_cursor: null } : { id: 'a', version: 2 })
    },
  })
  await setTaskDuplicateOf(client, 'workspace-1', { id: 'a', version: 1 }, 'target')
  await bulkSetTaskDuplicateOf(client, 'workspace-1', [{ id: 'a', version: 2 }], null)
  expect(requests).toEqual([
    { method: 'PATCH', path: '/api/v1/workspaces/workspace-1/tasks/a', body: { expected_version: 1, duplicate_of_id: 'target' } },
    { method: 'POST', path: '/api/v1/workspaces/workspace-1/tasks/bulk', body: { updates: [{ id: 'a', expected_version: 2, duplicate_of_id: null }] } },
  ])
})
