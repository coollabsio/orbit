import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from 'react-router'
import { queryKeys } from '@/api/queryKeys'
import { clearViewSessionEdits } from '@/features/views/useViewState'
import { DEFAULT_DISPLAY, PRESET_FILTERS } from '@/features/views/viewState'
import { ViewsPage } from '@/features/views/ViewsPage'
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

function renderAt(url: string, goTo?: string | string[]) {
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
            <Route path="views" element={<ViewsPage />} />
            <Route path="views/:viewId" element={<TasksPage />} />
            <Route path="views/:viewId/:taskId" element={<TasksPage />} />
          </Routes>
          <Location />
          {[goTo ?? []].flat().map((to) => <GoTo key={to} to={to} />)}
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

test('a failed task query shows in the list area, keeps the filter and display controls, and can retry', async () => {
  let down = true
  const label = { field: 'label', operator: 'includes_any', value: ['label-gone'] }
  const requests = serve(({ method, path, body }) => {
    if (method === 'GET' && path.endsWith('/view-preferences/all')) {
      return Response.json({ page_key: 'all', state: { filter: { op: 'and', children: [label] }, display: DEFAULT_DISPLAY }, state_error: null, updated_at: '2026-09-01T12:00:00Z' })
    }
    // the server rejects the stored filter, and is down once more for the retry below
    const filtered = (body as { filter?: { children: unknown[] } } | undefined)?.filter?.children.length
    if (path.endsWith('/tasks/query') && (filtered || down)) return problem(422, 'invalid_filter')
    return undefined
  })
  const page = renderAt('/tasks?workspace=workspace-1')

  expect(await page.findByText('Tasks unavailable')).toBeTruthy()
  // the header, filter bar, Display options and Advanced filter stay usable
  expect(page.getByText('All tasks')).toBeTruthy()
  fireEvent.click(page.getByRole('button', { name: 'Display options' }))
  expect(await page.findByRole('switch', { name: 'Show empty groups' })).toBeTruthy()
  await userEvent.keyboard('{Escape}')
  fireEvent.click(page.getByRole('button', { name: 'Filter tasks' }))
  expect(await page.findByRole('option', { name: 'Advanced filter' })).toBeTruthy()
  await userEvent.keyboard('{Escape}')

  // fixing the filter from the filter bar runs a new query, which still fails once: Retry recovers
  await userEvent.click(within(page.getByRole('toolbar', { name: 'Filters' })).getByRole('button', { name: 'Remove label filter' }))
  await waitFor(() => expect(taskQueries(requests).at(-1)?.body).toMatchObject({ filter: { op: 'and', children: [] } }))
  expect(await page.findByText('Tasks unavailable')).toBeTruthy()
  down = false
  await userEvent.click(page.getByRole('button', { name: 'Retry' }))
  expect(await page.findByText('Ship release')).toBeTruthy()
  expect(page.queryByText('Tasks unavailable')).toBeNull()
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

/** A saved view record as the server returns it (no stored state: the view shows the defaults). */
const viewRecord = (id: string, name: string, version = 1) => ({
  id, workspace_id: 'workspace-1', owner: { user_id: 'user-1', display_name: 'Ann' }, name, description: '',
  icon: null, color: null, visibility: 'personal', state: null, state_error: null, version, is_favorite: false, favorite_position: null,
  can_edit: true, created_at: '2026-09-01T12:00:00Z', updated_at: '2026-09-01T12:00:00Z',
})

/** A stored page preference: the page opens with this state instead of the defaults. */
const storedPreference = (pageKey: string, state: unknown) => Response.json({ page_key: pageKey, state, state_error: null, updated_at: '2026-09-01T12:00:00Z' })

test('Save view shows only once the page has a filter or a changed display', async () => {
  serve()
  const plain = renderAt('/tasks?workspace=workspace-1&view=overdue')
  expect(await plain.findByText('Ship release')).toBeTruthy()
  // the preset alone is how the page opens: nothing to save yet
  expect(plain.queryAllByRole('button', { name: 'Save view' })).toHaveLength(0)
  plain.unmount()

  serve(({ method, path }) => method === 'GET' && path.endsWith('/view-preferences/all')
    ? storedPreference('all', { filter: { op: 'and', children: [{ field: 'priority', operator: 'is', value: ['urgent'] }] }, display: DEFAULT_DISPLAY })
    : undefined)
  const filtered = renderAt('/tasks?workspace=workspace-1')
  expect(await filtered.findByText('Ship release')).toBeTruthy()
  // it sits at the end of the filter row, beside what it saves
  expect(within(filtered.getByRole('toolbar', { name: 'Filters' })).getAllByRole('button', { name: 'Save view' })).toHaveLength(1)
  filtered.unmount()

  // a changed display alone is a page preference, not a view: no filter row, no Save view
  serve(({ method, path }) => method === 'GET' && path.endsWith('/view-preferences/all')
    ? storedPreference('all', { filter: { op: 'and', children: [] }, display: { ...DEFAULT_DISPLAY, layout: 'board' } })
    : undefined)
  const arranged = renderAt('/tasks?workspace=workspace-1')
  expect(await arranged.findByText('Ship release')).toBeTruthy()
  expect(arranged.queryAllByRole('toolbar', { name: 'Filters' })).toHaveLength(0)
  expect(arranged.queryAllByRole('button', { name: 'Save view' })).toHaveLength(0)
})

test('Clear all removes the user filters, keeps the preset, and saves the empty filter', async () => {
  // a changed display must not keep Save view around once the filters are gone
  const requests = serve(({ method, path }) => method === 'GET' && path.endsWith('/view-preferences/preset:overdue')
    ? storedPreference('preset:overdue', { filter: { op: 'and', children: [{ field: 'priority', operator: 'is', value: ['urgent'] }] }, display: { ...DEFAULT_DISPLAY, group_by: 'priority' } })
    : undefined)
  const page = renderAt('/tasks?workspace=workspace-1&view=overdue')
  expect(await page.findByText('Ship release')).toBeTruthy()
  const toolbar = page.getByRole('toolbar', { name: 'Filters' })

  await userEvent.click(within(toolbar).getByRole('button', { name: 'Clear all' }))
  // nothing is left to clear or to save; the preset chip stays
  expect(within(toolbar).queryAllByRole('button', { name: 'Clear all' })).toHaveLength(0)
  expect(within(toolbar).queryAllByRole('button', { name: 'Save view' })).toHaveLength(0)
  expect(within(toolbar).getByText('Overdue')).toBeTruthy()
  await waitFor(() => expect(preferencePuts(requests).at(-1)?.body).toMatchObject({ state: { filter: { op: 'and', children: [] } } }))
})

test('Save view on a preset project page stores the preset and project as conditions', async () => {
  const display = { ...DEFAULT_DISPLAY, group_by: 'priority' }
  const requests = serve(({ method, path }) => {
    if (method === 'POST' && path.endsWith('/views')) return Response.json(viewRecord('view-new', 'Late launch'), { status: 201 })
    if (method === 'GET' && path.includes('/view-preferences/')) return storedPreference('project:project-1', { filter: { op: 'and', children: [{ field: 'priority', operator: 'is', value: ['high'] }] }, display })
    return undefined
  })
  const page = renderAt('/tasks?workspace=workspace-1&view=overdue&project=project-1')
  expect(await page.findByText('Ship release')).toBeTruthy()

  await userEvent.click(page.getByRole('button', { name: 'Save view' }))
  const dialog = await page.findByRole('dialog')
  await userEvent.type(within(dialog).getByRole('textbox', { name: 'Name' }), 'Late launch')
  await userEvent.click(within(dialog).getByRole('button', { name: 'Save view' }))

  await waitFor(() => expect(page.getByTestId('location').textContent).toBe('/views/view-new'))
  const posts = requests.filter(({ method, path }) => method === 'POST' && path.endsWith('/views'))
  expect(posts).toHaveLength(1)
  expect(posts[0]?.body).toMatchObject({
    name: 'Late launch',
    visibility: 'personal',
    state: {
      filter: { op: 'and', children: [...PRESET_FILTERS.overdue.children, { field: 'project', operator: 'is', value: ['project-1'] }, { field: 'priority', operator: 'is', value: ['high'] }] },
      display,
    },
  })
})

test('?save_view=1 opens the Save view dialog and drops the parameter', async () => {
  serve()
  const page = renderAt('/tasks?workspace=workspace-1&save_view=1')

  expect(await page.findByRole('heading', { name: 'Save view' })).toBeTruthy()
  await waitFor(() => expect(page.getByTestId('location').textContent).toBe('/tasks?workspace=workspace-1'))
})

/** An unsaved edit on an open saved view: Display options → Show empty groups. */
async function toggleEmptyGroups(page: ReturnType<typeof renderAt>) {
  fireEvent.click(page.getByRole('button', { name: 'Display options' }))
  await userEvent.click(await page.findByRole('switch', { name: 'Show empty groups' }))
  // close the (non-modal) popover, which hides the header from the accessibility tree while open
  await userEvent.keyboard('{Escape}')
}

/**
 * Another tab saved view-1 (version 1 → 2) after this tab opened it: this tab's Save gets a 409.
 * `versions` answers each GET of the view in turn; the last one repeats.
 */
async function openConflictingView(versions: Array<ReturnType<typeof viewRecord>>) {
  let reads = 0
  const requests = serve(({ method, path, body }) => {
    if (!path.endsWith('/views/view-1')) return undefined
    if (method === 'GET') return Response.json(versions[Math.min(reads++, versions.length - 1)])
    const expected = (body as { expected_version: number }).expected_version
    return expected === 2 ? Response.json({ ...viewRecord('view-1', 'Hot bugs', 3), state: (body as { state: unknown }).state }) : problem(409, 'conflict')
  })
  const page = renderAt('/views/view-1?workspace=workspace-1')
  expect(await page.findByText('Ship release')).toBeTruthy()

  await toggleEmptyGroups(page)
  // a saved view's edits are handled at the end of the filter row, where Save view sits on a page
  expect(page.getByRole('toolbar', { name: 'Filters' }).contains(page.getByRole('group', { name: 'Unsaved view changes' }))).toBe(true)
  await userEvent.click(within(page.getByRole('group', { name: 'Unsaved view changes' })).getByRole('button', { name: 'Update view' }))

  expect(await page.findByRole('alertdialog')).toBeTruthy()
  // the edits survive the conflict (behind the modal, so hidden from the accessibility tree)
  expect(page.getAllByRole('group', { name: 'Unsaved view changes', hidden: true })).toHaveLength(1)
  return { page, requests }
}

const viewPatches = (requests: Recorded[]) => requests.filter(({ method, path }) => method === 'PATCH' && path.endsWith('/views/view-1'))

test('a save that conflicts with another tab can overwrite with the fresh version', async () => {
  const { page, requests } = await openConflictingView([viewRecord('view-1', 'Hot bugs', 1), viewRecord('view-1', 'Hot bugs', 2)])

  await userEvent.click(page.getByRole('button', { name: 'Overwrite' }))

  await waitFor(() => expect(page.queryAllByRole('group', { name: 'Unsaved view changes', hidden: true })).toHaveLength(0))
  expect(viewPatches(requests).map(({ body }) => (body as { expected_version: number }).expected_version)).toEqual([1, 2])
  expect(viewPatches(requests)[1]?.body).toMatchObject({ state: { display: { show_empty_groups: true } } })
  expect(page.queryByRole('alertdialog')).toBeNull()
})

test('a save that conflicts with another tab can reload and drop the edits', async () => {
  const { page, requests } = await openConflictingView([viewRecord('view-1', 'Hot bugs', 1), viewRecord('view-1', 'Hot bugs triage', 2)])

  await userEvent.click(page.getByRole('button', { name: 'Reload' }))

  expect(await page.findByRole('heading', { name: 'Hot bugs triage' })).toBeTruthy()
  expect(page.queryAllByRole('group', { name: 'Unsaved view changes', hidden: true })).toHaveLength(0)
  expect(page.queryByRole('alertdialog')).toBeNull()
  expect(viewPatches(requests)).toHaveLength(1)
})

/** view-1 on a server that checks `expected_version` on every PATCH; `delay` holds each PATCH until released. */
function serveVersionedView({ canEdit = true, delay = false } = {}) {
  let record = { ...viewRecord('view-1', 'Hot bugs'), can_edit: canEdit }
  const releases: Array<() => void> = []
  const requests = serve(({ method, path, body }) => {
    if (!path.endsWith('/views/view-1')) return undefined
    if (method === 'GET') return Response.json(record)
    const { expected_version: expected, ...fields } = body as { expected_version: number }
    if (expected !== record.version) return problem(409, 'conflict')
    record = { ...record, ...fields, version: record.version + 1 }
    const response = Response.json(record)
    return delay ? new Promise<Response>((resolve) => { releases.push(() => resolve(response)) }) : response
  })
  return { requests, releases }
}

test('renaming a view with unsaved edits does not make the next save conflict', async () => {
  const { requests } = serveVersionedView()
  const page = renderAt('/views/view-1?workspace=workspace-1')
  expect(await page.findByText('Ship release')).toBeTruthy()
  await toggleEmptyGroups(page)

  fireEvent.click(page.getByRole('button', { name: 'View options' }))
  await userEvent.click(await page.findByRole('menuitem', { name: 'Edit view' }))
  const name = await page.findByRole('textbox', { name: 'Name' })
  await userEvent.clear(name)
  await userEvent.type(name, 'Hot bugs triage')
  await userEvent.click(page.getByRole('button', { name: 'Save changes' }))
  expect(await page.findByRole('heading', { name: 'Hot bugs triage' })).toBeTruthy()

  await userEvent.click(within(page.getByRole('group', { name: 'Unsaved view changes' })).getByRole('button', { name: 'Update view' }))

  await waitFor(() => expect(page.queryAllByRole('group', { name: 'Unsaved view changes', hidden: true })).toHaveLength(0))
  expect(viewPatches(requests).map(({ body }) => (body as { expected_version: number }).expected_version)).toEqual([1, 2])
  expect(page.queryAllByRole('alertdialog')).toHaveLength(0)
})

test('renaming a view from the Views page keeps its unsaved edits saveable without a conflict', async () => {
  let record = viewRecord('view-1', 'Hot bugs')
  const requests = serve(({ method, path, body }) => {
    if (method === 'GET' && path.endsWith('/views')) return Response.json([record])
    if (!path.endsWith('/views/view-1')) return undefined
    if (method === 'GET') return Response.json(record)
    const { expected_version: expected, ...fields } = body as { expected_version: number }
    if (expected !== record.version) return problem(409, 'conflict')
    record = { ...record, ...fields, version: record.version + 1 }
    return Response.json(record)
  })
  const page = renderAt('/views/view-1?workspace=workspace-1', ['/views', '/views/view-1'])
  expect(await page.findByText('Ship release')).toBeTruthy()
  await toggleEmptyGroups(page)

  fireEvent.click(page.getByRole('button', { name: 'Go to /views' }))
  fireEvent.click(await page.findByRole('button', { name: 'Hot bugs options' }))
  await userEvent.click(await page.findByRole('menuitem', { name: 'Edit view' }))
  const name = await page.findByRole('textbox', { name: 'Name' })
  await userEvent.clear(name)
  await userEvent.type(name, 'Hot bugs triage')
  await userEvent.click(page.getByRole('button', { name: 'Save changes' }))
  await waitFor(() => expect(viewPatches(requests)).toHaveLength(1))
  await waitFor(() => expect(page.queryByRole('dialog')).toBeNull())

  fireEvent.click(page.getByRole('button', { name: 'Go to /views/view-1' }))
  expect(await page.findByRole('heading', { name: 'Hot bugs triage' })).toBeTruthy()
  await userEvent.click(within(page.getByRole('group', { name: 'Unsaved view changes' })).getByRole('button', { name: 'Update view' }))

  await waitFor(() => expect(page.queryAllByRole('group', { name: 'Unsaved view changes', hidden: true })).toHaveLength(0))
  expect(viewPatches(requests).map(({ body }) => (body as { expected_version: number }).expected_version)).toEqual([1, 2])
  expect(page.queryAllByRole('alertdialog')).toHaveLength(0)
})

test('pressing Cmd+S twice during a slow save sends one save and no conflict', async () => {
  const { requests, releases } = serveVersionedView({ delay: true })
  const page = renderAt('/views/view-1?workspace=workspace-1')
  expect(await page.findByText('Ship release')).toBeTruthy()
  await toggleEmptyGroups(page)

  fireEvent.keyDown(document, { key: 's', metaKey: true })
  await waitFor(() => expect(releases).toHaveLength(1))
  fireEvent.keyDown(document, { key: 's', metaKey: true })
  await settle(50)
  releases[0]?.()

  await waitFor(() => expect(page.queryAllByRole('group', { name: 'Unsaved view changes', hidden: true })).toHaveLength(0))
  expect(viewPatches(requests)).toHaveLength(1)
  expect(page.queryAllByRole('alertdialog')).toHaveLength(0)
})

test('Cmd+S on a view the user cannot edit opens Save as new view without animation', async () => {
  serveVersionedView({ canEdit: false })
  const page = renderAt('/views/view-1?workspace=workspace-1')
  expect(await page.findByText('Ship release')).toBeTruthy()
  await toggleEmptyGroups(page)

  fireEvent.keyDown(document, { key: 's', ctrlKey: true })

  expect(await page.findByRole('heading', { name: 'Save as new view' })).toBeTruthy()
  expect(document.querySelector('[data-slot="dialog-content"]')?.hasAttribute('data-instant')).toBe(true)
})
