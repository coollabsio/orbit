import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { WorkspaceRecord } from '../../../api/generated/types.gen'
import { WorkspaceContext } from '../../workspaces/workspaceContext'
import type { Task, TaskViewState } from '../api/models'
import { TaskDetail } from './TaskDetail'

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
})

const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return (
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: mock(() => {}) }}>
          {children}
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>
  )
}

test('task due date uses the custom picker and can clear the saved value', async () => {
  let requestBody: unknown
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    requestBody = await (input as Request).json()
    return Response.json({
      id: 'task-1', workspace_id: workspace.id, project_id: 'project-1', identifier: 'ORB-1',
      title: 'Schedule me', descriptionJson: { type: 'doc', content: [] }, descriptionText: '', status_id: 'todo', position: 0, priority: 'none',
      assignee_ids: [], creator_id: 'user-1', labels: [], due_at: null, created_at: '', updated_at: '', version: 2,
    })
  }) as unknown as typeof fetch
  const task: Task = {
    id: 'task-1', identifier: 'ORB-1', title: 'Schedule me', descriptionJson: { type: 'doc', content: [] }, descriptionText: '', statusId: 'todo',
    position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
    labels: [], attachments: [], dueAt: '2030-01-02T12:30:00.000Z', parentId: null, subIssueTotal: 0, subIssueDone: 0, duplicateOfTaskId: null, duplicateIds: [], referencedBy: [], createdAt: '', updatedAt: '',
    comments: [], activity: [], version: 1,
  }
  const state: TaskViewState = { currentUserId: 'user-1', users: [], statuses: [], labels: [], tasks: [task] }
  const view = render(<TaskDetail task={task} project={undefined} projects={[]} state={state} onBack={() => {}} onOpenTask={() => {}} />, { wrapper: Wrapper })

  expect(view.container.querySelector('input[type="datetime-local"]')).toBeNull()
  fireEvent.click(view.getByRole('button', { name: 'Due date' }))
  const clearButton = view.getByRole('button', { name: 'Clear' }) as HTMLButtonElement
  expect(clearButton.disabled).toBe(false)
  fireEvent.click(clearButton)

  await waitFor(() => expect(requestBody).toEqual({ expected_version: 1, due_at: null }))
})
