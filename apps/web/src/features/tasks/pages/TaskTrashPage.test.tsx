import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { TaskTrashPage } from './TaskTrashPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const workspace = { id: 'workspace-1', name: 'Orbit', role: 'owner' as const, version: 1 }
const trashed = (id: string, title: string, count: number) => ({
  id, workspace_id: 'workspace-1', project_id: 'project-1', status_id: 'todo', title, description: '', position: 0, priority: 'none',
  assignee_ids: [], creator_id: 'user-1', label_ids: [], created_at: '', updated_at: '', deleted_at: '2026-09-27T10:00:00Z', version: 2,
  duplicate_of: null, blocked: false, parent_task_id: null, parent: null, sub_issue_count: 0, sub_issue_closed_count: 0, trashed_descendant_count: count,
})

test('a trashed parent shows how many sub-issues went to trash with it', async () => {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const path = new URL((input as Request).url).pathname
    if (path.endsWith('/tasks/trash')) {
      return Response.json({ items: [trashed('task-0012', 'Checkout redesign', 2), trashed('task-0040', 'Solo task', 0), trashed('task-0041', 'One child', 1)], next_cursor: null })
    }
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}><TaskTrashPage /></WorkspaceContext.Provider>
    </QueryClientProvider>,
  )
  expect(await view.findByText('+2 sub-issues')).toBeTruthy()
  expect(view.getByText('+1 sub-issue')).toBeTruthy()
  expect(view.queryAllByText(/^\+\d+ sub-issues?$/)).toHaveLength(2)
})
