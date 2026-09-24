import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { Task, TaskStatusDef, TaskViewState } from '@/features/tasks/api/models'
import { TaskDetail } from './TaskDetail'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const workspace = { id: 'workspace-1', name: 'Orbit', role: 'owner' as const, version: 1 }
const project = { id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'ORB', color: '#e0457b', created_at: '', updated_at: '', version: 1 }
const todo: TaskStatusDef = { id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#888', category: 'unstarted', position: 0, version: 1 }
const duplicateStatus: TaskStatusDef = { id: 'dup', projectId: 'project-1', name: 'Duplicate', description: '', color: '#8b8f98', category: 'duplicate', position: 1, version: 1 }
const task: Task = {
  id: 'task-3f2a', identifier: 'ORB-3F2A', title: "Can't log in on iPad", description: '', statusId: 'todo',
  position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1', labels: [], attachments: [],
  dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 1, duplicateOf: null, blocked: false,
}
const state: TaskViewState = { currentUserId: 'user-1', users: [], statuses: [todo, duplicateStatus], labels: [], tasks: [task] }
const record = (id: string, title: string) => ({
  id, workspace_id: 'workspace-1', project_id: 'project-1', status_id: 'todo', title, description: '', position: 0,
  priority: 'none', assignee_ids: [], creator_id: 'user-1', label_ids: [], created_at: '', updated_at: '', version: 1,
  duplicate_of: null, blocked: false,
})
const relation = (id: string, type: string, direction: string, other: { id: string; title: string }) => ({
  id, type, direction, created_at: `2026-09-23T10:00:0${id.at(-1)}Z`,
  task: { id: other.id, project_id: 'project-1', title: other.title, status_id: 'todo' },
})

type Call = { method: string; path: string; body?: unknown }
function api(calls: Call[], relations: unknown[] = []) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const path = new URL(request.url).pathname
    const body = request.method === 'POST' || request.method === 'PATCH' ? await request.json() : undefined
    calls.push({ method: request.method, path, body })
    if (path.endsWith('/relations') && request.method === 'GET') return Response.json(relations)
    if (path.endsWith('/relations') && request.method === 'POST') {
      return Response.json(relation('rel-9', 'blocks', 'incoming', { id: 'task-77aa', title: 'Auth token refresh' }), { status: 201 })
    }
    if (request.method === 'DELETE') return new Response(null, { status: 204 })
    if (path.endsWith('/github-links')) return Response.json([])
    if (path.endsWith('/projects')) return Response.json({ items: [project], next_cursor: null })
    if (path.endsWith('/tasks')) return Response.json({ items: [record('task-3f2a', "Can't log in on iPad"), record('task-77aa', 'Auth token refresh')], next_cursor: null })
    if (request.method === 'PATCH') return Response.json({ ...record('task-3f2a', "Can't log in on iPad"), version: 2 })
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
}
const writes = (calls: Call[]) => calls.filter((call) => call.method !== 'GET')

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return <QueryClientProvider client={client}><MemoryRouter><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider></MemoryRouter></QueryClientProvider>
}

function renderDetail(overrides: Partial<Task> = {}, onOpenTask: (taskId: string) => void = () => {}) {
  return render(<TaskDetail task={{ ...task, ...overrides }} project={project} state={state} onBack={() => {}} onOpenTask={onOpenTask} />, { wrapper: Wrapper })
}

test('a duplicate shows a banner that opens the canonical task and unmarks it', async () => {
  const calls: Call[] = []
  api(calls)
  const opened: string[] = []
  const view = renderDetail(
    { statusId: 'dup', duplicateOf: { id: 'task-91c0', projectId: 'project-1', title: 'Login fails on Safari' } },
    (id) => opened.push(id),
  )
  const banner = await view.findByRole('note', { name: 'Duplicate of ORB-91C0' })
  expect(banner.textContent).toContain('Login fails on Safari')
  // already a duplicate when the page opened: no entrance animation
  expect(banner.className).not.toContain('animate-relation-enter')
  fireEvent.click(within(banner).getByRole('button', { name: /Login fails on Safari/ }))
  expect(opened).toEqual(['task-91c0'])
  const unmark = within(banner).getByRole('button', { name: 'Unmark' })
  expect(unmark.className).toContain('active:scale-[0.97]')
  fireEvent.click(unmark)
  await waitFor(() => expect(writes(calls)).toEqual([
    { method: 'PATCH', path: '/api/v1/workspaces/workspace-1/tasks/task-3f2a', body: { expected_version: 1, duplicate_of_id: null } },
  ]))
})

test('relations are grouped in a fixed order; a row opens its task and × removes the relation', async () => {
  const calls: Call[] = []
  api(calls, [
    relation('rel-1', 'related', 'outgoing', { id: 'task-12cd', title: 'Safari cookie policy' }),
    relation('rel-2', 'blocks', 'incoming', { id: 'task-77aa', title: 'Auth token refresh' }),
    relation('rel-3', 'duplicate', 'incoming', { id: 'task-0b9e', title: 'Safari login loop' }),
    relation('rel-4', 'blocks', 'outgoing', { id: 'task-55ee', title: 'Release notes' }),
  ])
  const opened: string[] = []
  const view = renderDetail({}, (id) => opened.push(id))
  const section = await view.findByRole('region', { name: 'Relations' })
  expect(within(section).getAllByRole('group').map((group) => group.getAttribute('aria-label'))).toEqual(['Blocked by', 'Blocks', 'Related', 'Duplicated by'])
  fireEvent.click(within(section).getByRole('button', { name: /Auth token refresh/ }))
  expect(opened).toEqual(['task-77aa'])
  const remove = await within(section).findByRole('button', { name: 'Remove relation to ORB-77AA' })
  expect(remove.className).toContain('hover-fine:opacity-0')
  expect(remove.className).toContain('active:scale-[0.97]')
  fireEvent.click(remove)
  await waitFor(() => expect(writes(calls)).toEqual([
    { method: 'DELETE', path: '/api/v1/workspaces/workspace-1/tasks/task-3f2a/relations/rel-2', body: undefined },
  ]))
})

test('the relations section stays hidden without relations', async () => {
  const calls: Call[] = []
  api(calls, [])
  const view = renderDetail()
  await waitFor(() => expect(calls.some((call) => call.path.endsWith('/relations'))).toBe(true))
  expect(view.queryByRole('region', { name: 'Relations' })).toBeNull()
})

test('choosing the Duplicate status opens the picker instead of saving a status', async () => {
  const calls: Call[] = []
  api(calls)
  const view = renderDetail()
  fireEvent.click(view.getByRole('button', { name: /Todo$/ }))
  await userEvent.click(await view.findByRole('menuitem', { name: /Duplicate$/ }, { timeout: 5000 }))
  expect(await view.findByRole('dialog', { name: 'Mark ORB-3F2A as duplicate of…' })).toBeTruthy()
  expect(writes(calls)).toEqual([])
}, 20000)

test('Add relation → Blocked by… links the picked task and never offers the task itself', async () => {
  const calls: Call[] = []
  api(calls)
  const view = renderDetail()
  // the text fields re-mount their children (Attach, Add relation) once GitHub links load; wait for the editable title
  await view.findByRole('textbox', { name: 'Task title' })
  fireEvent.click(view.getByRole('button', { name: /Add relation/ }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Blocked by…' }, { timeout: 5000 }))
  const picker = await view.findByRole('dialog', { name: 'ORB-3F2A is blocked by…' })
  const option = await within(picker).findByRole('option', { name: /Auth token refresh/ })
  expect(within(picker).queryByRole('option', { name: /Can't log in on iPad/ })).toBeNull()
  fireEvent.click(option)
  await waitFor(() => expect(writes(calls)).toEqual([
    { method: 'POST', path: '/api/v1/workspaces/workspace-1/tasks/task-3f2a/relations', body: { type: 'blocked_by', task_id: 'task-77aa' } },
  ]))
}, 20000)
