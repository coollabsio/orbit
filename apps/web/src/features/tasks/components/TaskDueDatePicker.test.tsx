import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { WorkspaceRecord } from '@/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { Task, TaskViewState } from '@/features/tasks/api/models'
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
      title: 'Schedule me', description: '', status_id: 'todo', position: 0, priority: 'none',
      assignee_ids: [], creator_id: 'user-1', labels: [], due_at: null, created_at: '', updated_at: '', version: 2,
    })
  }) as unknown as typeof fetch
  const task: Task = {
    id: 'task-1', identifier: 'ORB-1', title: 'Schedule me', description: '', statusId: 'todo',
    position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
    labels: [], attachments: [], dueAt: '2030-01-02T12:30:00.000Z', createdAt: '', updatedAt: '',
    comments: [], activity: [], version: 1,
  }
  const state: TaskViewState = { currentUserId: 'user-1', users: [], statuses: [], labels: [], tasks: [task] }
  const view = render(<TaskDetail task={task} project={undefined} state={state} onBack={() => {}} />, { wrapper: Wrapper })

  expect(view.container.querySelector('input[type="datetime-local"]')).toBeNull()
  fireEvent.click(view.getByRole('button', { name: 'Due date' }))
  const clearButton = view.getByRole('button', { name: 'Clear' }) as HTMLButtonElement
  expect(clearButton.disabled).toBe(false)
  fireEvent.click(clearButton)

  await waitFor(() => expect(requestBody).toEqual({ expected_version: 1, due_start_at: null, due_at: null }))
})

test('This week saves a Monday through Sunday due-date range', async () => {
  let requestBody: Record<string, unknown> | undefined
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    requestBody = await (input as Request).json()
    return Response.json({
      id: 'task-1', workspace_id: workspace.id, project_id: 'project-1', status_id: 'todo',
      title: 'Schedule me', description: '', position: 0, priority: 'none', assignee_ids: [],
      creator_id: 'user-1', label_ids: [], due_start_at: requestBody?.due_start_at,
      due_at: requestBody?.due_at, created_at: '', updated_at: '', version: 2,
    })
  }) as unknown as typeof fetch
  const task: Task = {
    id: 'task-1', identifier: 'ORB-1', title: 'Schedule me', description: '', statusId: 'todo',
    position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
    labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
  }
  const state: TaskViewState = { currentUserId: 'user-1', users: [], statuses: [], labels: [], tasks: [task] }
  const view = render(<TaskDetail task={task} project={undefined} state={state} onBack={() => {}} />, { wrapper: Wrapper })

  fireEvent.click(view.getByRole('button', { name: 'Due date' }))
  fireEvent.click(view.getByRole('button', { name: 'This week' }))
  fireEvent.click(view.getByRole('button', { name: 'Done' }))

  await waitFor(() => expect(requestBody).toBeDefined())
  const start = new Date(requestBody?.due_start_at as string)
  const end = new Date(requestBody?.due_at as string)
  expect(start.getDay()).toBe(1)
  expect(end.getDay()).toBe(0)
  expect(Math.round((end.getTime() - start.getTime()) / 86_400_000)).toBe(6)
})

test('task source appears in properties and saves a separate URL', async () => {
  let patchBody: Record<string, unknown> | undefined
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'PATCH') {
      patchBody = await request.json() as Record<string, unknown>
      return Response.json({ id: 'task-1', source_url: patchBody.source_url })
    }
    return Response.json([])
  }) as unknown as typeof fetch
  const task: Task = {
    id: 'task-1', identifier: 'ORB-1', title: 'Linked task', description: 'Issue details', sourceUrl: 'https://github.com/acme/repo/issues/12', statusId: 'todo',
    position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
    labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
  }
  const state: TaskViewState = { currentUserId: 'user-1', users: [], statuses: [], labels: [], tasks: [task] }
  const view = render(<TaskDetail task={task} project={undefined} state={state} onBack={() => {}} />, { wrapper: Wrapper })

  const source = view.getByRole('link', { name: 'github.com' }) as HTMLAnchorElement
  expect(source.href).toBe('https://github.com/acme/repo/issues/12')
  expect(view.getByText('Issue details')).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Edit source' }))
  const input = await view.findByLabelText('Source URL')
  fireEvent.change(input, { target: { value: 'https://example.com/issue/12' } })
  expect((input as HTMLInputElement).value).toBe('https://example.com/issue/12')
  fireEvent.click(view.getByRole('button', { name: 'Save' }))

  await waitFor(() => expect(patchBody).toEqual({ expected_version: 1, source_url: 'https://example.com/issue/12' }))
})
