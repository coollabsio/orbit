import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { WorkspaceRecord } from '../../../api/generated/types.gen'
import { GeneralPage } from '../../settings/GeneralPage'
import { WorkspaceContext } from '../../workspaces/workspaceContext'
import type { Task, TaskViewState } from '../api/models'
import { ProjectRail } from './ProjectRail'
import { TaskDetail } from './TaskDetail'

const originalFetch = globalThis.fetch
const originalPrompt = window.prompt
const originalConfirm = window.confirm

afterEach(() => {
  globalThis.fetch = originalFetch
  window.prompt = originalPrompt
  window.confirm = originalConfirm
})

const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }

function wrapper(selectWorkspace = mock(() => {})) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return {
    selectWorkspace,
    Wrapper: ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace }}>
            {children}
          </WorkspaceContext.Provider>
        </MemoryRouter>
      </QueryClientProvider>
    ),
  }
}

function failure() {
  return Response.json({
    type: 'about:blank', title: 'Write failed', status: 500, detail: 'offline', code: 'write_failed',
    instance: '/write', request_id: 'request-1',
  }, { status: 500, headers: { 'content-type': 'application/problem+json' } })
}

test('workspace creation retry repeats form clearing and workspace selection', async () => {
  let calls = 0
  const bodies: unknown[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    bodies.push(await (input as Request).json())
    calls += 1
    return calls === 1 ? failure() : Response.json({ id: 'workspace-2', name: 'Second', role: 'owner', version: 0 }, { status: 201 })
  }) as unknown as typeof fetch
  const selectWorkspace = mock(() => {})
  const { Wrapper } = wrapper(selectWorkspace)
  const view = render(<GeneralPage />, { wrapper: Wrapper })
  const user = userEvent.setup()

  await user.type(view.getByLabelText('New workspace'), 'Second')
  expect((view.getByLabelText('New workspace') as HTMLInputElement).value).toBe('Second')
  await user.click(view.getByRole('button', { name: 'Create workspace' }))
  await view.findByRole('alert')
  await user.click(view.getByRole('button', { name: 'Retry' }))

  await waitFor(() => expect(calls).toBe(2))
  await waitFor(() => expect(selectWorkspace).toHaveBeenCalledWith('workspace-2'))
  expect(bodies).toEqual([{ name: 'Second' }, { name: 'Second' }])
  expect((view.getByLabelText('New workspace') as HTMLInputElement).value).toBe('')
})

test('project creation retry selects the created project', async () => {
  let calls = 0
  globalThis.fetch = (async () => {
    calls += 1
    return calls === 1 ? failure() : Response.json({
      id: 'project-2', workspace_id: workspace.id, name: 'Second', key: 'SECON', color: '#8b5cf6',
      created_at: '', updated_at: '', version: 0,
    }, { status: 201 })
  }) as unknown as typeof fetch
  window.prompt = () => 'Second'
  const onSelect = mock(() => {})
  const { Wrapper } = wrapper()
  const view = render(<ProjectRail projects={[]} projectId={null} onSelect={onSelect} />, { wrapper: Wrapper })

  fireEvent.click(view.getByRole('button', { name: 'New project' }))
  await view.findByRole('alert')
  fireEvent.click(view.getByRole('button', { name: 'Retry' }))

  await waitFor(() => expect(onSelect).toHaveBeenCalledWith('project-2'))
})

test('task deletion retry returns to the task list after success', async () => {
  let calls = 0
  globalThis.fetch = (async () => {
    calls += 1
    return calls === 1 ? failure() : new Response(null, { status: 204 })
  }) as unknown as typeof fetch
  window.confirm = () => true
  const onBack = mock(() => {})
  const { Wrapper } = wrapper()
  const task: Task = {
    id: 'task-1', identifier: 'ORB-1', title: 'Delete me', description: '', statusId: 'todo',
    position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
    labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
  }
  const state: TaskViewState = { currentUserId: 'user-1', users: [], statuses: [], labels: [], tasks: [task] }
  const view = render(<TaskDetail task={task} project={undefined} state={state} onBack={onBack} />, { wrapper: Wrapper })

  fireEvent.click(view.getByRole('button', { name: 'Delete task' }))
  await view.findByRole('alert')
  fireEvent.click(view.getByRole('button', { name: 'Retry' }))

  await waitFor(() => expect(onBack).toHaveBeenCalledTimes(1))
})
