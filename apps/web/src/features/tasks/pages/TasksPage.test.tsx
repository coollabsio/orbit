import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from 'react-router'
import { queryKeys } from '@/api/queryKeys'
import { clearViewSessionEdits } from '@/features/views/useViewState'
import { PRESET_FILTERS } from '@/features/views/viewState'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { TasksPage } from './TasksPage'

const originalFetch = globalThis.fetch
afterEach(() => {
  // unmount first: unmounting flushes waiting autosaves, which must still hit the mocked fetch
  cleanup()
  globalThis.fetch = originalFetch
  localStorage.clear()
  clearViewSessionEdits()
})

type Recorded = { method: string; path: string; body: unknown }

const problem = (status: number, code: string) => Response.json({
  type: 'about:blank', title: 'Request failed', status, code, detail: code, instance: '/api', request_id: 'request-1',
}, { status, headers: { 'content-type': 'application/problem+json' } })

const task = (id: string, title: string, projectId = 'project-1') => ({
  id, workspace_id: 'workspace-1', project_id: projectId, status_id: projectId === 'project-1' ? 'todo' : 'todo-2',
  title, description: '', priority: 'none', position: 1, assignee_ids: [], label_ids: [], creator_id: 'user-1', version: 1,
  identifier: id.toUpperCase(), created_at: '2026-09-01T12:00:00Z', updated_at: '2026-09-01T12:00:00Z',
})

const TASKS = [task('task-1', 'Ship release'), task('task-2', 'Write launch notes', 'project-2')]

/** A workspace with two projects; `override` answers first, then these defaults. */
function serve(override: (request: Recorded) => Response | Promise<Response> | undefined = () => undefined) {
  const requests: Recorded[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const text = await request.text()
    const entry = { method: request.method, path: decodeURIComponent(new URL(request.url).pathname), body: text ? JSON.parse(text) : undefined }
    requests.push(entry)
    const answer = await override(entry)
    if (answer) return answer
    const { path } = entry
    if (path === '/api/v1/workspaces') return Response.json([{ id: 'workspace-1', name: 'Alpha', role: 'owner', version: 1 }])
    if (path.endsWith('/auth/me')) return Response.json({ id: 'user-1', display_name: 'Ann', email: 'ann@example.com' })
    if (path.endsWith('/projects')) return Response.json({ items: [
      { id: 'project-1', name: 'Launch', key: 'LCH', color: '#e0457b', version: 1 },
      { id: 'project-2', name: 'Growth', key: 'GRW', color: '#3366ff', version: 1 },
    ], next_cursor: null })
    if (path.endsWith('/project-1/statuses')) return Response.json({ items: [{ id: 'todo', project_id: 'project-1', name: 'Todo', category: 'unstarted', color: '#888888', position: 0, version: 1 }], next_cursor: null })
    if (path.endsWith('/project-2/statuses')) return Response.json({ items: [{ id: 'todo-2', project_id: 'project-2', name: 'Todo', category: 'unstarted', color: '#888888', position: 0, version: 1 }], next_cursor: null })
    if (path.endsWith('/tasks/query')) return Response.json({ items: TASKS, next_cursor: null })
    if (path.includes('/view-preferences/')) return entry.method === 'PUT'
      ? Response.json({ page_key: 'all', state: (entry.body as { state: unknown }).state, state_error: null, updated_at: '2026-09-01T12:00:00Z' })
      : problem(404, 'task_resource_not_found')
    const detail = TASKS.find(({ id }) => path.endsWith(`/tasks/${id}`))
    if (detail) return Response.json(detail)
    if (path.endsWith('/github-links') || path.endsWith('/relations')) return Response.json([])
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  return requests
}

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

/** Moves to another URL in the same router, as a sidebar link would. */
function GoTo({ to }: { to: string }) {
  const navigate = useNavigate()
  return <button type="button" onClick={() => navigate(to)}>Go to {to}</button>
}

function renderAt(url: string, goTo?: string) {
  // retries stay on (the hooks set them), but run at once
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, retryDelay: 0 }, mutations: { retry: false } } })
  client.setQueryData(queryKeys.workspaces, [{ id: 'workspace-1', name: 'Alpha', role: 'owner', version: 1 }])
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[url]}>
        <WorkspaceProvider>
          <Routes>
            <Route path="tasks" element={<TasksPage />} />
            <Route path="tasks/:taskId" element={<TasksPage />} />
            <Route path="views/:viewId" element={<TasksPage />} />
            <Route path="views/:viewId/:taskId" element={<TasksPage />} />
          </Routes>
          <Location />
          {goTo ? <GoTo to={goTo} /> : null}
        </WorkspaceProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

const taskQueries = (requests: Recorded[]) => requests.filter(({ path }) => path.endsWith('/tasks/query'))
const preferencePuts = (requests: Recorded[]) => requests.filter(({ method, path }) => method === 'PUT' && path.includes('/view-preferences/'))
/** Lets time pass (debounces, retries) with React updates flushed inside `act`. */
const settle = (ms: number) => act(() => new Promise<void>((resolve) => setTimeout(resolve, ms)))

test('a saved view that returns 404 shows "View not found" and never loads tasks', async () => {
  const requests = serve(({ path }) => path.endsWith('/views/view-9') ? problem(404, 'view_not_found') : undefined)
  const page = renderAt('/views/view-9?workspace=workspace-1')

  expect(await page.findByText('View not found')).toBeTruthy()
  await settle(50)
  expect(taskQueries(requests)).toHaveLength(0)
  expect(page.queryByText('Ship release')).toBeNull()
})

test('a saved view that fails otherwise shows "View unavailable" and never loads tasks', async () => {
  const requests = serve(({ path }) => path.endsWith('/views/view-9') ? problem(500, 'internal_error') : undefined)
  const page = renderAt('/views/view-9?workspace=workspace-1')

  expect(await page.findByText('View unavailable')).toBeTruthy()
  expect(page.queryByText('View not found')).toBeNull()
  await settle(50)
  expect(taskQueries(requests)).toHaveLength(0)
})

test('a page whose preference cannot be loaded shows an error instead of loading forever', async () => {
  const requests = serve(({ method, path }) => method === 'GET' && path.includes('/view-preferences/') ? problem(500, 'internal_error') : undefined)
  const page = renderAt('/tasks?workspace=workspace-1')

  expect(await page.findByText('Tasks unavailable')).toBeTruthy()
  expect(page.getByText('The server could not load the settings for this page.')).toBeTruthy()
  expect(taskQueries(requests)).toHaveLength(0)
})

test('a saved view loads its tasks with the view filter and titles the page', async () => {
  const filter = { op: 'and', children: [{ field: 'priority', operator: 'is', value: ['high'] }] }
  const requests = serve(({ path }) => path.endsWith('/views/view-1') ? Response.json({
    id: 'view-1', workspace_id: 'workspace-1', owner: { user_id: 'user-1', display_name: 'Ann' }, name: 'Hot bugs', description: '',
    icon: null, color: null, visibility: 'personal', state_error: null, version: 1, is_favorite: false, favorite_position: null,
    can_edit: true, created_at: '2026-09-01T12:00:00Z', updated_at: '2026-09-01T12:00:00Z',
    state: { filter, display: { layout: 'list', group_by: 'status', sub_group_by: 'none', order_by: 'title', order_direction: 'asc', properties: ['id'], show_completed: 'all', show_empty_groups: false } },
  }) : undefined)
  const page = renderAt('/views/view-1?workspace=workspace-1')

  expect(await page.findByText('Ship release')).toBeTruthy()
  expect(page.getByText('Hot bugs')).toBeTruthy()
  expect(taskQueries(requests)[0]?.body).toMatchObject({ filter, order_by: 'title', order_direction: 'asc' })
  expect(requests.some(({ path }) => path.includes('/view-preferences/'))).toBeFalse()
})

test('typing in quick search filters locally and never saves a preference', async () => {
  const requests = serve()
  const page = renderAt('/tasks?workspace=workspace-1')
  expect(await page.findByText('Ship release')).toBeTruthy()
  const queriesBefore = taskQueries(requests).length

  await userEvent.type(page.getByRole('searchbox', { name: 'Search tasks' }), 'launch')

  await waitFor(() => expect(page.queryByText('Ship release')).toBeNull())
  expect(page.getByText('Write launch notes')).toBeTruthy()
  // longer than the 500 ms autosave debounce
  await settle(800)
  expect(preferencePuts(requests)).toHaveLength(0)
  expect(taskQueries(requests)).toHaveLength(queriesBefore)
})

test('closing a task returns to the page it was opened from', async () => {
  serve()
  const all = renderAt('/tasks?workspace=workspace-1')
  fireEvent.click(await all.findByText('Write launch notes'))
  await settle(50)
  await waitFor(() => expect(all.getByTestId('location').textContent).toBe('/tasks/task-2?workspace=workspace-1'))
  await all.findByRole('button', { name: 'Close task' })
  fireEvent.keyDown(document, { key: 'Escape' })
  // not `?project=project-2`: that is another page, with its own filters and layout
  await waitFor(() => expect(all.getByTestId('location').textContent).toBe('/tasks?workspace=workspace-1'))
  cleanup()

  const project = renderAt('/tasks?workspace=workspace-1&project=project-1')
  fireEvent.click(await project.findByText('Ship release'))
  await settle(50)
  await waitFor(() => expect(project.getByTestId('location').textContent).toBe('/tasks/task-1?workspace=workspace-1'))
  await project.findByRole('button', { name: 'Close task' })
  fireEvent.keyDown(document, { key: 'Escape' })
  await waitFor(() => expect(project.getByTestId('location').textContent).toBe('/tasks?workspace=workspace-1&project=project-1'))
})

test('a saved view opens and closes tasks under its own path', async () => {
  serve(({ path }) => path.endsWith('/views/view-1') ? Response.json({
    id: 'view-1', workspace_id: 'workspace-1', owner: { user_id: 'user-1', display_name: 'Ann' }, name: 'Hot bugs', description: '',
    icon: null, color: null, visibility: 'personal', state: null, state_error: null, version: 1, is_favorite: false, favorite_position: null,
    can_edit: true, created_at: '2026-09-01T12:00:00Z', updated_at: '2026-09-01T12:00:00Z',
  }) : undefined)
  const page = renderAt('/views/view-1?workspace=workspace-1')
  fireEvent.click(await page.findByText('Ship release'))
  await settle(50)
  await waitFor(() => expect(page.getByTestId('location').textContent).toBe('/views/view-1/task-1?workspace=workspace-1'))
  await page.findByRole('button', { name: 'Close task' })
  fireEvent.keyDown(document, { key: 'Escape' })
  await waitFor(() => expect(page.getByTestId('location').textContent).toBe('/views/view-1?workspace=workspace-1'))
})

test('a preset on a project page queries the preset conditions and the project', async () => {
  const requests = serve()
  const page = renderAt('/tasks?workspace=workspace-1&view=overdue&project=project-1')

  expect(await page.findByText('Ship release')).toBeTruthy()
  expect(requests.some(({ method, path }) => method === 'GET' && path.endsWith('/view-preferences/preset:overdue'))).toBeTrue()
  expect(taskQueries(requests)[0]?.body).toMatchObject({ filter: {
    op: 'and', children: [...PRESET_FILTERS.overdue.children, { field: 'project', operator: 'is', value: ['project-1'] }],
  } })
})

test('switching to a page whose preference is still loading shows loading, not the previous page\'s tasks', async () => {
  let releaseOverdue: (() => void) | undefined
  const requests = serve(({ method, path }) => method === 'GET' && path.endsWith('/view-preferences/preset:overdue')
    ? new Promise<Response>((resolve) => { releaseOverdue = () => resolve(problem(404, 'task_resource_not_found')) })
    : undefined)
  const overdue = '/tasks?workspace=workspace-1&view=overdue'
  const page = renderAt('/tasks?workspace=workspace-1', overdue)
  expect(await page.findByText('Ship release')).toBeTruthy()
  const queriesBefore = taskQueries(requests).length

  fireEvent.click(page.getByRole('button', { name: `Go to ${overdue}` }))

  expect(await page.findByText('Loading tasks')).toBeTruthy()
  expect(page.queryByText('Ship release')).toBeNull()
  expect(taskQueries(requests)).toHaveLength(queriesBefore)

  releaseOverdue?.()
  // the page title and the preset's fixed filter chip
  expect((await page.findAllByText('Overdue')).length).toBe(2)
  expect(await page.findByText('Ship release')).toBeTruthy()
  expect(taskQueries(requests)).toHaveLength(queriesBefore + 1)
})
