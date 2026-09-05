import { expect, test } from 'bun:test'
import { createApiClient } from '../../../api/client'
import { nextTaskCursor, taskListPage } from './tasks'

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
