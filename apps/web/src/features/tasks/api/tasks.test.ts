import { expect, test } from 'bun:test'
import { createApiClient } from '../../../api/client'
import { nextTaskCursor, taskListAllPages, taskListPage } from './tasks'

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
