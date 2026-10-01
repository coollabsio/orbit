import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { queryKeys } from '@/api/queryKeys'
import { useLogout } from '@/features/auth/api'
import type { SavedView } from '@/features/views/api/views'
import { clearViewSessionEdits, rebaseViewSessionEdit, useViewState, type ViewSource } from './useViewState'
import { DEFAULT_DISPLAY, PRESET_FILTERS, defaultViewState, type FilterGroup, type ViewState } from './viewState'

const originalFetch = globalThis.fetch
afterEach(() => {
  // unmount first: unmounting flushes waiting autosaves, which must still hit the mocked fetch
  cleanup()
  globalThis.fetch = originalFetch
  localStorage.clear()
  clearViewSessionEdits()
})

type Recorded = { method: string; path: string; body: unknown }

function serve(respond: (request: Recorded) => Response | Promise<Response>) {
  const requests: Recorded[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const text = await request.text()
    const entry = { method: request.method, path: decodeURIComponent(new URL(request.url).pathname), body: text ? JSON.parse(text) : undefined }
    requests.push(entry)
    return respond(entry)
  }) as unknown as typeof fetch
  return requests
}

const problem = (status: number, code: string) => Response.json({
  type: 'about:blank', title: 'Request failed', status, code, detail: code, instance: '/api', request_id: 'request-1',
}, { status, headers: { 'content-type': 'application/problem+json' } })

const preferenceRecord = (state: ViewState | null) => Response.json({ page_key: 'all', state, state_error: null, updated_at: '2026-09-01T12:00:00Z' })

const savedView = (overrides: Partial<SavedView> = {}): SavedView => ({
  id: 'view-1', workspace_id: 'workspace-1', owner: { user_id: 'user-1', display_name: 'Ann' },
  name: 'Bugs', description: '', icon: null, color: null, visibility: 'personal',
  state: defaultViewState() as unknown as SavedView['state'], state_error: null, version: 3,
  is_favorite: false, favorite_position: null, can_edit: true, can_change_visibility: true, created_at: '2026-09-01T12:00:00Z', updated_at: '2026-09-01T12:00:00Z',
  ...overrides,
})

const testClient = () => new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
function withClient(client: QueryClient) {
  return ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
}

const allTasks: ViewSource = { kind: 'page', pageKey: 'all', preset: null, projectId: null }
const bugs: FilterGroup = { op: 'and', children: [{ field: 'label', operator: 'includes_any', value: ['label-bug'] }] }
const puts = (requests: Recorded[]) => requests.filter(({ method }) => method === 'PUT')

test('a page without a stored preference starts from defaults and autosaves edits after a debounce', async () => {
  const requests = serve(({ method }) => method === 'GET' ? problem(404, 'task_resource_not_found') : preferenceRecord(null))
  const view = renderHook(() => useViewState('workspace-1', allTasks), { wrapper: withClient(testClient()) })
  expect(view.result.current.isLoading).toBeTrue()
  await waitFor(() => expect(view.result.current.isLoading).toBeFalse())
  expect(view.result.current.state).toEqual(defaultViewState())
  expect(view.result.current).toMatchObject({ dirty: false, canEdit: true, conflict: false, stateError: null })

  act(() => view.result.current.setDisplay({ layout: 'board', group_by: 'none' }))
  act(() => view.result.current.setFilter(bugs))

  expect(view.result.current.state).toEqual({ filter: bugs, display: { ...DEFAULT_DISPLAY, layout: 'board', group_by: 'status' } })
  expect(puts(requests)).toHaveLength(0)
  await waitFor(() => expect(puts(requests)).toHaveLength(1), { timeout: 2000 })
  expect(puts(requests)[0]).toEqual({
    method: 'PUT', path: '/api/v1/workspaces/workspace-1/view-preferences/all', body: { state: view.result.current.state },
  })
})

test('a stored preference wins and scoped pages send the preset and project with it', async () => {
  const stored: ViewState = { filter: bugs, display: { ...DEFAULT_DISPLAY, layout: 'timeline' } }
  localStorage.setItem('orbit:task_layout', 'board')
  const requests = serve(() => preferenceRecord(stored))
  const source: ViewSource = { kind: 'page', pageKey: 'preset:mine', preset: 'mine', projectId: 'project-1' }
  const view = renderHook(() => useViewState('workspace-1', source), { wrapper: withClient(testClient()) })
  await waitFor(() => expect(view.result.current.isLoading).toBeFalse())

  expect(requests[0]?.path).toBe('/api/v1/workspaces/workspace-1/view-preferences/preset:mine')
  expect(view.result.current.state).toEqual(stored)
  expect(view.result.current.effective).toEqual({
    op: 'and', children: [...PRESET_FILTERS.mine.children, { field: 'project', operator: 'is', value: ['project-1'] }, ...bugs.children],
  })
  // only the `all` page migrates, and only when it has no stored preference
  expect(localStorage.getItem('orbit:task_layout')).toBe('board')
})

test('the first visit moves legacy localStorage preferences to the server, then deletes the workspace ones', async () => {
  localStorage.setItem('orbit:task_preferences:workspace-1', JSON.stringify({
    sort: 'created', statusFilter: 'unstarted:todo', assigneeFilter: null, unassignedFilter: true,
    labelFilter: null, priorityFilter: 'high', searchFilter: 'release',
  }))
  localStorage.setItem('orbit:task_layout', 'timeline')
  let finishPut: (() => void) | undefined
  const requests = serve(({ method }) => method === 'GET'
    ? problem(404, 'task_resource_not_found')
    : new Promise<Response>((resolve) => { finishPut = () => resolve(preferenceRecord(null)) }))
  const view = renderHook(() => useViewState('workspace-1', allTasks), { wrapper: withClient(testClient()) })
  await waitFor(() => expect(view.result.current.isLoading).toBeFalse())

  expect(view.result.current.state).toEqual({
    filter: { op: 'and', children: [
      { field: 'status', operator: 'is', value: ['unstarted:todo'] },
      { field: 'assignee', operator: 'is_empty' },
      { field: 'priority', operator: 'is', value: ['high'] },
    ] },
    display: { ...DEFAULT_DISPLAY, layout: 'timeline', order_by: 'created', order_direction: 'desc' },
  })
  await waitFor(() => expect(puts(requests)).toHaveLength(1), { timeout: 2000 })
  expect(puts(requests)[0]?.body).toEqual({ state: view.result.current.state })
  expect(localStorage.getItem('orbit:task_preferences:workspace-1')).not.toBeNull()

  finishPut?.()
  await waitFor(() => expect(localStorage.getItem('orbit:task_preferences:workspace-1')).toBeNull())
  // the layout key is global: other workspaces still migrate it
  expect(localStorage.getItem('orbit:task_layout')).toBe('timeline')
})

test('a failed autosave keeps the local state', async () => {
  const requests = serve(({ method }) => method === 'GET' ? problem(404, 'task_resource_not_found') : problem(503, 'unavailable'))
  const view = renderHook(() => useViewState('workspace-1', allTasks), { wrapper: withClient(testClient()) })
  await waitFor(() => expect(view.result.current.isLoading).toBeFalse())
  act(() => view.result.current.setFilter(bugs))
  await waitFor(() => expect(puts(requests)).toHaveLength(1), { timeout: 2000 })
  await new Promise((resolve) => setTimeout(resolve, 50))
  expect(view.result.current.state.filter).toEqual(bugs)
  // the retry waits for the backoff (1 s), not a short fixed timer
  expect(puts(requests)).toHaveLength(1)
})

test('saved-view edits stay unsaved for the session until saved or discarded', async () => {
  let record = savedView()
  const requests = serve(({ method, body }) => {
    if (method === 'PATCH') record = { ...record, state: (body as { state: SavedView['state'] }).state, version: record.version + 1 }
    return Response.json(record)
  })
  const client = testClient()
  const source: ViewSource = { kind: 'view', viewId: 'view-1' }
  const first = renderHook(() => useViewState('workspace-1', source), { wrapper: withClient(client) })
  await waitFor(() => expect(first.result.current.isLoading).toBeFalse())
  expect(first.result.current).toMatchObject({ dirty: false, canEdit: true, view: record })

  act(() => first.result.current.setFilter(bugs))
  expect(first.result.current.dirty).toBeTrue()
  first.unmount()

  const second = renderHook(() => useViewState('workspace-1', source), { wrapper: withClient(client) })
  await waitFor(() => expect(second.result.current.isLoading).toBeFalse())
  expect(second.result.current.dirty).toBeTrue()
  expect(second.result.current.state.filter).toEqual(bugs)

  act(() => second.result.current.discard())
  expect(second.result.current.dirty).toBeFalse()
  expect(second.result.current.state).toEqual(defaultViewState())

  act(() => second.result.current.setFilter(bugs))
  act(() => second.result.current.setFilter(defaultViewState().filter))
  expect(second.result.current.dirty).toBeFalse()

  act(() => second.result.current.setFilter(bugs))
  await act(() => second.result.current.save())
  expect(requests.filter(({ method }) => method === 'PATCH')).toEqual([{
    method: 'PATCH', path: '/api/v1/workspaces/workspace-1/views/view-1',
    body: { expected_version: 3, state: { filter: bugs, display: DEFAULT_DISPLAY } },
  }])
  expect(second.result.current.dirty).toBeFalse()
  expect(second.result.current.view?.version).toBe(4)
  expect(second.result.current.state.filter).toEqual(bugs)
})

test('a version conflict is reported, and overwrite re-saves with the fresh version', async () => {
  let serverVersion = 3
  let patches = 0
  const requests = serve(({ method, body }) => {
    if (method !== 'PATCH') return Response.json(savedView({ version: serverVersion }))
    patches += 1
    if (patches === 1) return problem(409, 'conflict')
    return Response.json(savedView({ version: serverVersion + 1, state: (body as { state: SavedView['state'] }).state }))
  })
  const view = renderHook(() => useViewState('workspace-1', { kind: 'view', viewId: 'view-1' }), { wrapper: withClient(testClient()) })
  await waitFor(() => expect(view.result.current.isLoading).toBeFalse())
  serverVersion = 7

  act(() => view.result.current.setDisplay({ layout: 'board' }))
  await act(() => view.result.current.save())
  expect(view.result.current.conflict).toBeTrue()
  expect(view.result.current.dirty).toBeTrue()

  await act(() => view.result.current.save({ overwrite: true }))
  expect(requests.filter(({ method }) => method === 'PATCH').map(({ body }) => (body as { expected_version: number }).expected_version)).toEqual([3, 7])
  expect(view.result.current.conflict).toBeFalse()
  expect(view.result.current.dirty).toBeFalse()
  expect(view.result.current.state.display.layout).toBe('board')
})

test('discarding after a conflict drops the edits and reloads the view', async () => {
  let serverVersion = 3
  const requests = serve(({ method }) => method === 'PATCH' ? problem(409, 'conflict') : Response.json(savedView({ version: serverVersion })))
  const view = renderHook(() => useViewState('workspace-1', { kind: 'view', viewId: 'view-1' }), { wrapper: withClient(testClient()) })
  await waitFor(() => expect(view.result.current.isLoading).toBeFalse())
  serverVersion = 5
  act(() => view.result.current.setFilter(bugs))
  await act(() => view.result.current.save())
  expect(view.result.current.conflict).toBeTrue()

  act(() => view.result.current.discard())

  expect(view.result.current).toMatchObject({ conflict: false, dirty: false })
  await waitFor(() => expect(view.result.current.view?.version).toBe(5))
  expect(requests.filter(({ method }) => method === 'GET')).toHaveLength(2)
})

test('an unreadable saved view opens with defaults and can be saved to reset it', async () => {
  const requests = serve(({ method, body }) => Response.json(method === 'PATCH'
    ? savedView({ version: 4, state: (body as { state: SavedView['state'] }).state })
    : savedView({ state: null, state_error: 'unknown field `colour`', can_edit: false })))
  const view = renderHook(() => useViewState('workspace-1', { kind: 'view', viewId: 'view-1' }), { wrapper: withClient(testClient()) })
  await waitFor(() => expect(view.result.current.isLoading).toBeFalse())

  expect(view.result.current.state).toEqual(defaultViewState())
  expect(view.result.current).toMatchObject({ stateError: 'unknown field `colour`', dirty: true, canEdit: false })
  await act(() => view.result.current.save())
  expect(requests.at(-1)).toMatchObject({ method: 'PATCH', body: { expected_version: 3, state: defaultViewState() } })
})

test('a preference that fails to load keeps the page loading and never autosaves defaults over it', async () => {
  const requests = serve(({ method }) => method === 'GET' ? problem(500, 'internal') : preferenceRecord(null))
  // the hook retries non-404 errors itself; no delay between attempts keeps the test fast
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, retryDelay: 0 }, mutations: { retry: false } } })
  const view = renderHook(() => useViewState('workspace-1', allTasks), { wrapper: withClient(client) })
  await waitFor(() => expect(client.getQueryState(queryKeys.viewPreference('workspace-1', 'all'))?.status).toBe('error'))
  expect(requests.filter(({ method }) => method === 'GET')).toHaveLength(4)
  expect(view.result.current.isLoading).toBeTrue()

  act(() => view.result.current.setFilter(bugs))
  await new Promise((resolve) => setTimeout(resolve, 700))
  expect(puts(requests)).toHaveLength(0)
  expect(view.result.current.isLoading).toBeTrue()
})

test('saving checks against the version the edits started from, not a newer refetch', async () => {
  let record = savedView()
  const requests = serve(({ method, body }) => {
    if (method !== 'PATCH') return Response.json(record)
    const patch = body as { expected_version: number; state: SavedView['state'] }
    if (patch.expected_version !== record.version) return problem(409, 'conflict')
    record = { ...record, state: patch.state, version: record.version + 1 }
    return Response.json(record)
  })
  const client = testClient()
  const view = renderHook(() => useViewState('workspace-1', { kind: 'view', viewId: 'view-1' }), { wrapper: withClient(client) })
  await waitFor(() => expect(view.result.current.isLoading).toBeFalse())

  act(() => view.result.current.setFilter(bugs))
  // another tab saves the view; the realtime event refetches it under the unsaved edits
  record = { ...record, name: 'Bugs (renamed)', version: 4 }
  await act(() => client.invalidateQueries({ queryKey: queryKeys.workspace('workspace-1') }))
  await waitFor(() => expect(view.result.current.view?.version).toBe(4))
  expect(view.result.current.dirty).toBeTrue()

  await act(() => view.result.current.save())
  expect(requests.filter(({ method }) => method === 'PATCH').map(({ body }) => (body as { expected_version: number }).expected_version)).toEqual([3])
  expect(view.result.current.conflict).toBeTrue()
  expect(view.result.current.dirty).toBeTrue()
  expect(view.result.current.state.filter).toEqual(bugs)
})

test('edits made while a save is in flight are checked against the version that save produced', async () => {
  let record = savedView()
  let release: (() => void) | undefined
  const requests = serve(({ method, body }) => {
    if (method !== 'PATCH') return Response.json(record)
    const patch = body as { expected_version: number; state: SavedView['state'] }
    if (patch.expected_version !== record.version) return problem(409, 'conflict')
    record = { ...record, state: patch.state, version: record.version + 1 }
    const response = Response.json(record)
    if (record.version > 4) return response
    return new Promise<Response>((resolve) => { release = () => resolve(response) })
  })
  const view = renderHook(() => useViewState('workspace-1', { kind: 'view', viewId: 'view-1' }), { wrapper: withClient(testClient()) })
  await waitFor(() => expect(view.result.current.isLoading).toBeFalse())

  act(() => view.result.current.setFilter(bugs))
  let saving: Promise<void> | undefined
  act(() => { saving = view.result.current.save() })
  await waitFor(() => expect(release).toBeDefined())
  act(() => view.result.current.setDisplay({ layout: 'board' }))
  release!()
  await act(() => saving!)
  expect(view.result.current.dirty).toBeTrue()

  await act(() => view.result.current.save())
  expect(requests.filter(({ method }) => method === 'PATCH').map(({ body }) => (body as { expected_version: number }).expected_version)).toEqual([3, 4])
  expect(view.result.current).toMatchObject({ conflict: false, dirty: false })
  expect(view.result.current.view?.version).toBe(5)
})

test('signing out drops unsaved saved-view edits', async () => {
  serve(({ method }) => method === 'POST' ? new Response(null, { status: 204 }) : Response.json(savedView()))
  const client = testClient()
  const source: ViewSource = { kind: 'view', viewId: 'view-1' }
  const view = renderHook(() => ({ controller: useViewState('workspace-1', source), logout: useLogout() }), { wrapper: withClient(client) })
  await waitFor(() => expect(view.result.current.controller.isLoading).toBeFalse())
  act(() => view.result.current.controller.setFilter(bugs))
  expect(view.result.current.controller.dirty).toBeTrue()

  await act(() => view.result.current.logout.mutateAsync())
  await waitFor(() => expect(view.result.current.controller.isLoading).toBeFalse())
  expect(view.result.current.controller.dirty).toBeFalse()
  expect(view.result.current.controller.state).toEqual(defaultViewState())
})

const patchVersions = (requests: Recorded[]) =>
  requests.filter(({ method }) => method === 'PATCH').map(({ body }) => (body as { expected_version: number }).expected_version)

test('a second save while one is in flight is ignored instead of conflicting', async () => {
  let record = savedView()
  let release: (() => void) | undefined
  const requests = serve(({ method, body }) => {
    if (method !== 'PATCH') return Response.json(record)
    const patch = body as { expected_version: number; state: SavedView['state'] }
    if (patch.expected_version !== record.version) return problem(409, 'conflict')
    record = { ...record, state: patch.state, version: record.version + 1 }
    const response = Response.json(record)
    return new Promise<Response>((resolve) => { release = () => resolve(response) })
  })
  const view = renderHook(() => useViewState('workspace-1', { kind: 'view', viewId: 'view-1' }), { wrapper: withClient(testClient()) })
  await waitFor(() => expect(view.result.current.isLoading).toBeFalse())

  act(() => view.result.current.setFilter(bugs))
  let first: Promise<void> | undefined
  let second: Promise<void> | undefined
  act(() => {
    first = view.result.current.save()
    second = view.result.current.save()
  })
  await waitFor(() => expect(release).toBeDefined())
  release!()
  await act(() => Promise.all([first, second]))

  expect(patchVersions(requests)).toEqual([3])
  expect(view.result.current).toMatchObject({ conflict: false, dirty: false })
})

test('overwrite with nothing left to save closes the conflict', async () => {
  const requests = serve(({ method }) => method === 'PATCH' ? problem(409, 'conflict') : Response.json(savedView()))
  const view = renderHook(() => useViewState('workspace-1', { kind: 'view', viewId: 'view-1' }), { wrapper: withClient(testClient()) })
  await waitFor(() => expect(view.result.current.isLoading).toBeFalse())
  act(() => view.result.current.setDisplay({ layout: 'board' }))
  await act(() => view.result.current.save())
  expect(view.result.current.conflict).toBeTrue()
  // back at the saved state: no edits remain
  act(() => view.result.current.setDisplay({ layout: 'list' }))

  await act(() => view.result.current.save({ overwrite: true }))

  expect(view.result.current).toMatchObject({ conflict: false, dirty: false })
  expect(patchVersions(requests)).toEqual([3])
})

test('rebasing after our own metadata update moves only edits from that version', async () => {
  let record = savedView()
  const requests = serve(({ method, body }) => {
    if (method !== 'PATCH') return Response.json(record)
    const patch = body as { expected_version: number; state: SavedView['state'] }
    if (patch.expected_version !== record.version) return problem(409, 'conflict')
    record = { ...record, state: patch.state, version: record.version + 1 }
    return Response.json(record)
  })
  const client = testClient()
  const view = renderHook(() => useViewState('workspace-1', { kind: 'view', viewId: 'view-1' }), { wrapper: withClient(client) })
  await waitFor(() => expect(view.result.current.isLoading).toBeFalse())
  act(() => view.result.current.setFilter(bugs))
  // Edit view renamed it (3 → 4); a rebase from another version is ignored
  record = { ...record, name: 'Bugs (renamed)', version: 4 }
  act(() => rebaseViewSessionEdit('workspace-1', 'view-1', 2, 4))
  act(() => rebaseViewSessionEdit('workspace-1', 'view-1', 3, 4))

  await act(() => view.result.current.save())

  expect(patchVersions(requests)).toEqual([4])
  expect(view.result.current).toMatchObject({ conflict: false, dirty: false })
})

test('a refetch that lands the old preference before the debounced PUT does not lose the edit', async () => {
  let stored: ViewState | null = null
  let releasePut: (() => void) | undefined
  const requests = serve(({ method, body }) => {
    if (method === 'GET') return stored ? preferenceRecord(stored) : problem(404, 'task_resource_not_found')
    // the server stores the new state only once the held PUT is released
    return new Promise<Response>((resolve) => {
      releasePut = () => {
        stored = (body as { state: ViewState }).state
        resolve(preferenceRecord(stored))
      }
    })
  })
  const client = testClient()
  const first = renderHook(() => useViewState('workspace-1', allTasks), { wrapper: withClient(client) })
  await waitFor(() => expect(first.result.current.isLoading).toBeFalse())

  act(() => first.result.current.setFilter(bugs))
  await waitFor(() => expect(puts(requests)).toHaveLength(1), { timeout: 2000 })
  // e.g. a workspace-wide invalidation: the refetch still reads the old (missing) preference
  await act(() => client.invalidateQueries({ queryKey: queryKeys.workspace('workspace-1') }))
  await waitFor(() => expect(client.getQueryData(queryKeys.viewPreference('workspace-1', 'all'))).toBeNull())
  await act(async () => { releasePut?.() })

  await waitFor(() => expect((client.getQueryData(queryKeys.viewPreference('workspace-1', 'all')) as ViewState | null)?.filter).toEqual(bugs))
  first.unmount()
  // coming back to the page reseeds from the cache
  const again = renderHook(() => useViewState('workspace-1', allTasks), { wrapper: withClient(client) })
  await waitFor(() => expect(again.result.current.isLoading).toBeFalse())
  expect(again.result.current.state.filter).toEqual(bugs)
})
