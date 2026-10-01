import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, waitFor } from '@testing-library/react'
import { render } from '@/test/render'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { NewTaskDialog } from './NewTaskDialog'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
  window.localStorage.clear()
})

const workspace = { id: 'workspace-1', name: 'Orbit', role: 'owner' as const, version: 1 }
const project = { id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'ORB', color: '#e0457b', created_at: '', updated_at: '', version: 1, auto_close_parent: true, auto_close_sub_issues: true }
const status = (id: string, name: string, category: string, position: number) => ({ id, project_id: 'project-1', name, description: '', color: '#888', category, position, version: 1 })

/** Records the bodies of the create requests; each one answers with a new task. */
function api(created: unknown[], uploads: string[] = []) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    if (request.method === 'POST' && url.pathname.endsWith('/tasks')) {
      const body = await request.json()
      created.push(body)
      return Response.json({
        ...body, id: `task-000${created.length}`, workspace_id: 'workspace-1', position: 0, creator_id: 'user-1', created_at: '', updated_at: '',
        version: 1, duplicate_of: null, blocked: false, parent_task_id: null, parent: null, sub_issue_count: 0, sub_issue_closed_count: 0, auto_closed: [],
      })
    }
    if (request.method === 'POST' && url.pathname.endsWith('/labels')) return Response.json({ id: 'label-1', workspace_id: 'workspace-1', name: 'bug', color: '#8b5cf6', version: 1, created_at: '', updated_at: '' })
    if (request.method === 'POST' && url.pathname.endsWith('/attachments')) {
      uploads.push(url.pathname)
      return Response.json([])
    }
    if (url.pathname.endsWith('/projects')) return Response.json({ items: [project], next_cursor: null })
    if (url.pathname.endsWith('/statuses')) return Response.json({ items: [status('doing', 'Doing', 'started', 0), status('todo', 'Todo', 'unstarted', 0)], next_cursor: null })
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
}

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return <QueryClientProvider client={client}><MemoryRouter><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider></MemoryRouter></QueryClientProvider>
}

test('creates the task with the defaults and opens it', async () => {
  const created: unknown[] = []
  api(created)
  const opened: string[] = []
  const view = render(<NewTaskDialog defaults={{ priority: 'high' }} onClose={() => {}} onOpenTask={(task) => opened.push(task.id)} />, { wrapper: Wrapper })
  // no status default: the project's first unstarted status
  await view.findByRole('button', { name: 'Status: Todo' })
  await userEvent.type(view.getByLabelText('Task title'), '  Fix login  ')
  await userEvent.click(view.getByRole('button', { name: 'Create task' }))
  await waitFor(() => expect(opened).toEqual(['task-0001']))
  expect(created).toEqual([{ title: 'Fix login', description: '', project_id: 'project-1', status_id: 'todo', priority: 'high', assignee_ids: [], label_ids: [], due_at: null, due_start_at: null }])
})

test('"Create more" keeps the dialog and the properties for the next task', async () => {
  window.localStorage.setItem('orbit:new_task_create_more', '1')
  const created: Array<{ title: string; status_id: string }> = []
  api(created)
  const opened: string[] = []
  const view = render(<NewTaskDialog defaults={{ project_id: 'project-1', status_id: 'doing' }} onClose={() => {}} onOpenTask={(task) => opened.push(task.id)} />, { wrapper: Wrapper })
  await view.findByRole('button', { name: 'Status: Doing' })
  const title = view.getByLabelText('Task title') as HTMLInputElement
  await userEvent.type(title, 'First{Control>}{Enter}{/Control}')
  await view.findByText('Created')
  expect(title.value).toBe('')
  expect(opened).toEqual([])
  await userEvent.type(title, 'Second{Control>}{Enter}{/Control}')
  await waitFor(() => expect(created.map((body) => [body.title, body.status_id])).toEqual([['First', 'doing'], ['Second', 'doing']]))
  // the notice opens the last created task
  await userEvent.click(await view.findByRole('button', { name: 'ORB-0002' }))
  expect(opened).toEqual(['task-0002'])
})

test('an empty title creates nothing', async () => {
  const created: unknown[] = []
  api(created)
  const view = render(<NewTaskDialog onClose={() => {}} onOpenTask={() => {}} />, { wrapper: Wrapper })
  await view.findByRole('button', { name: 'Status: Todo' })
  await userEvent.type(view.getByLabelText('Task title'), '   {Control>}{Enter}{/Control}')
  expect((view.getByRole('button', { name: 'Create task' }) as HTMLButtonElement).disabled).toBe(true)
  expect(created).toEqual([])
})

test('attached files upload to the task once it exists', async () => {
  const created: unknown[] = []
  const uploads: string[] = []
  api(created, uploads)
  const opened: string[] = []
  const view = render(<NewTaskDialog onClose={() => {}} onOpenTask={(task) => opened.push(task.id)} />, { wrapper: Wrapper })
  await view.findByRole('button', { name: 'Status: Todo' })
  await userEvent.upload(view.getByLabelText('Attach files'), new File(['proof'], 'proof.txt', { type: 'text/plain' }))
  await view.findByText('proof.txt')
  await userEvent.type(view.getByLabelText('Task title'), 'With a file{Control>}{Enter}{/Control}')
  await waitFor(() => expect(opened).toEqual(['task-0001']))
  expect(uploads).toEqual(['/api/v1/workspaces/workspace-1/tasks/task-0001/attachments'])
})

test('creating a label inside the dialog does not create the task', async () => {
  const created: unknown[] = []
  api(created)
  const view = render(<NewTaskDialog onClose={() => {}} onOpenTask={() => {}} />, { wrapper: Wrapper })
  await view.findByRole('button', { name: 'Status: Todo' })
  await userEvent.type(view.getByLabelText('Task title'), 'Has a title')
  fireEvent.click(view.getByRole('button', { name: 'Add label' }))
  const name = await view.findByLabelText('New label name') as HTMLInputElement
  await userEvent.type(name, 'bug{Enter}')
  // the label form clears once its request has finished
  await waitFor(() => expect(name.value).toBe(''))
  expect(created).toEqual([])
})

test('Mod+Shift+Enter creates the task and keeps the dialog, with "Create more" off', async () => {
  const created: Array<{ title: string }> = []
  api(created)
  const opened: string[] = []
  const view = render(<NewTaskDialog onClose={() => {}} onOpenTask={(task) => opened.push(task.id)} />, { wrapper: Wrapper })
  await view.findByRole('button', { name: 'Status: Todo' })
  const title = view.getByLabelText('Task title') as HTMLInputElement
  await userEvent.type(title, 'First{Control>}{Shift>}{Enter}{/Shift}{/Control}')
  await view.findByText('Created')
  expect(title.value).toBe('')
  expect(opened).toEqual([])
  expect(created.map((body) => body.title)).toEqual(['First'])
})
