import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { ProjectRecord, WorkspaceRecord } from '../../../api/generated/types.gen'
import { WorkspaceContext } from '../../workspaces/workspaceContext'
import type { Task, TaskStatusDef } from '../api/models'
import { NewTaskModal } from './NewTaskModal'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return (
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
          {children}
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>
  )
}

const projects: ProjectRecord[] = [
  { id: 'project-1', workspace_id: 'workspace-1', name: 'Alpha', key: 'ALP', color: '#5e6ad2', created_at: '', updated_at: '', version: 0 },
]

const statuses: TaskStatusDef[] = [
  { id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#8b8f98', category: 'unstarted', position: 0, version: 0 },
  { id: 'doing', projectId: 'project-1', name: 'In Progress', description: '', color: '#f2c94c', category: 'started', position: 1, version: 0 },
]

const parent: Task = {
  id: 'task-1', identifier: 'ORB-1', title: 'Parent', descriptionJson: { type: 'doc', content: [] }, descriptionText: '',
  statusId: 'todo', position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
  labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
  parentId: null, subIssueTotal: 0, subIssueDone: 0, duplicateOfTaskId: null, duplicateIds: [], referencedBy: [],
}

function fetchMock(bodies: unknown[]) {
  return (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'POST') {
      bodies.push(await request.json())
      return Response.json({ id: 'new-task', title: 'x', project_id: 'project-1', status_id: 'todo', version: 0 }, { status: 201 })
    }
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
}

test('creates an issue with the chosen fields and closes without navigating', async () => {
  const bodies: unknown[] = []
  globalThis.fetch = fetchMock(bodies)
  const onClose = mock(() => {})
  const onCreated = mock(() => {})
  const view = render(
    <NewTaskModal workspaceId="workspace-1" projects={projects} statuses={statuses} users={[]} labels={[]} onClose={onClose} onCreated={onCreated} />,
    { wrapper: Wrapper },
  )

  fireEvent.change(view.getByLabelText('Issue title'), { target: { value: 'Ship it' } })
  // The priority picker writes through to the create body.
  fireEvent.click(view.getByText('Priority'))
  fireEvent.click(view.getByText('Urgent'))
  fireEvent.click(view.getByRole('button', { name: 'Create issue' }))

  await waitFor(() => expect(bodies.length).toBe(1))
  expect(bodies[0]).toMatchObject({ title: 'Ship it', project_id: 'project-1', status_id: 'todo', priority: 'urgent', parent_id: null })
  await waitFor(() => expect(onCreated).toHaveBeenCalledTimes(1))
  expect(onClose).toHaveBeenCalledTimes(1)
})

test('the sub-issue variant sets parent_id and labels its action', async () => {
  const bodies: unknown[] = []
  globalThis.fetch = fetchMock(bodies)
  const view = render(
    <NewTaskModal workspaceId="workspace-1" projects={projects} statuses={statuses} users={[]} labels={[]} parent={parent} defaultProjectId={parent.projectId} onClose={() => {}} />,
    { wrapper: Wrapper },
  )

  expect(view.getByText(/Sub-issue of ORB-1/)).toBeTruthy()
  fireEvent.change(view.getByLabelText('Issue title'), { target: { value: 'A child' } })
  fireEvent.click(view.getByRole('button', { name: 'Create sub-issue' }))

  await waitFor(() => expect(bodies.length).toBe(1))
  expect(bodies[0]).toMatchObject({ title: 'A child', parent_id: 'task-1', project_id: 'project-1', status_id: 'todo' })
})

test('"Create more" keeps the modal open and clears the title for the next issue', async () => {
  const bodies: unknown[] = []
  globalThis.fetch = fetchMock(bodies)
  const onClose = mock(() => {})
  const view = render(
    <NewTaskModal workspaceId="workspace-1" projects={projects} statuses={statuses} users={[]} labels={[]} onClose={onClose} />,
    { wrapper: Wrapper },
  )

  fireEvent.click(view.getByLabelText('Create more'))
  const title = view.getByLabelText('Issue title') as HTMLInputElement
  fireEvent.change(title, { target: { value: 'First' } })
  fireEvent.click(view.getByRole('button', { name: 'Create issue' }))

  await waitFor(() => expect(bodies.length).toBe(1))
  expect(onClose).not.toHaveBeenCalled()
  await waitFor(() => expect((view.getByLabelText('Issue title') as HTMLInputElement).value).toBe(''))
})
