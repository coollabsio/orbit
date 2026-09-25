import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { createApiClient } from '@/api/client'
import { ApiProblem } from '@/api/problem'
import { queryKeys } from '@/api/queryKeys'
import { defaultViewState, type ViewState } from '@/features/views/viewState'
import {
  savePreference,
  savedViewsQueryOptions,
  useReorderFavorites,
  useSavedView,
  useSetFavorite,
  useUpdateView,
  useViewPreference,
  viewPreferenceQueryOptions,
  type SavedView,
} from './views'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

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

const savedView = (overrides: Partial<SavedView> = {}): SavedView => ({
  id: 'view-1', workspace_id: 'workspace-1', owner: { user_id: 'user-1', display_name: 'Ann' },
  name: 'Bugs', description: '', icon: null, color: null, visibility: 'personal',
  state: defaultViewState() as unknown as SavedView['state'], state_error: null, version: 3,
  is_favorite: false, favorite_position: null, can_edit: true, created_at: '2026-09-01T12:00:00Z', updated_at: '2026-09-01T12:00:00Z',
  ...overrides,
})

const testClient = () => new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
function withClient(client: QueryClient) {
  return ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
}

test('a page without a stored preference reads as null', async () => {
  let path = ''
  const client = createApiClient({
    fetch: async (request) => {
      path = decodeURIComponent(new URL(request.url).pathname)
      return problem(404, 'task_resource_not_found')
    },
  })
  expect(await viewPreferenceQueryOptions('workspace-1', 'project:project-1', client).queryFn()).toBeNull()
  expect(path).toBe('/api/v1/workspaces/workspace-1/view-preferences/project:project-1')
})

test('a stored preference returns its state, and unreadable state reads as null', async () => {
  const state: ViewState = { ...defaultViewState(), display: { ...defaultViewState().display, layout: 'board' } }
  let body: unknown = { page_key: 'all', state, state_error: null, updated_at: '2026-09-01T12:00:00Z' }
  const client = createApiClient({ fetch: async () => Response.json(body) })
  expect(await viewPreferenceQueryOptions('workspace-1', 'all', client).queryFn()).toEqual(state)
  body = { page_key: 'all', state: null, state_error: 'unknown field `colour`', updated_at: '2026-09-01T12:00:00Z' }
  expect(await viewPreferenceQueryOptions('workspace-1', 'all', client).queryFn()).toBeNull()
})

test('other preference failures still fail the query', async () => {
  const client = createApiClient({ fetch: async () => problem(500, 'internal_error') })
  const error = await viewPreferenceQueryOptions('workspace-1', 'all', client).queryFn().catch((caught: unknown) => caught)
  expect(error).toBeInstanceOf(ApiProblem)
  expect((error as ApiProblem).status).toBe(500)
})

test('saving a preference PUTs the whole state under its page key', async () => {
  const requests: Recorded[] = []
  const client = createApiClient({
    fetch: async (request) => {
      requests.push({ method: request.method, path: decodeURIComponent(new URL(request.url).pathname), body: await request.json() })
      return Response.json({ page_key: 'preset:mine', state: defaultViewState(), state_error: null, updated_at: '2026-09-01T12:00:00Z' })
    },
  })
  await savePreference('workspace-1', 'preset:mine', defaultViewState(), client)
  expect(requests).toEqual([{ method: 'PUT', path: '/api/v1/workspaces/workspace-1/view-preferences/preset:mine', body: { state: defaultViewState() } }])
})

test('the views list comes from GET /views', async () => {
  const client = createApiClient({ fetch: async () => Response.json([savedView()]) })
  expect((await savedViewsQueryOptions('workspace-1', client).queryFn()).map(({ id }) => id)).toEqual(['view-1'])
})

test('a missing view fails at once instead of retrying', async () => {
  const requests = serve(() => problem(404, 'task_resource_not_found'))
  const view = renderHook(() => useSavedView('workspace-1', 'view-9'), { wrapper: withClient(new QueryClient()) })
  await waitFor(() => expect(view.result.current.isError).toBeTrue())
  expect(requests).toHaveLength(1)
})

test('updating a view sends the expected version and refreshes the cached view', async () => {
  const state: ViewState = { filter: { op: 'and', children: [{ field: 'priority', operator: 'is', value: ['high'] }] }, display: defaultViewState().display }
  const requests = serve(({ body }) => Response.json(savedView({ version: 4, state: (body as { state: SavedView['state'] }).state })))
  const client = testClient()
  client.setQueryData(queryKeys.view('workspace-1', 'view-1'), savedView())
  const view = renderHook(() => useUpdateView('workspace-1'), { wrapper: withClient(client) })

  await act(async () => { await view.result.current.mutateAsync({ viewId: 'view-1', body: { expected_version: 3, state } }) })

  expect(requests).toEqual([{ method: 'PATCH', path: '/api/v1/workspaces/workspace-1/views/view-1', body: { expected_version: 3, state } }])
  expect(client.getQueryData<SavedView>(queryKeys.view('workspace-1', 'view-1'))?.version).toBe(4)
})

test('favoriting flips the star at once and uses PUT or DELETE', async () => {
  let release: (() => void) | undefined
  const requests = serve(() => new Promise<Response>((resolve) => { release = () => resolve(new Response(null, { status: 204 })) }))
  const client = testClient()
  client.setQueryData(queryKeys.views('workspace-1'), [savedView()])
  client.setQueryData(queryKeys.view('workspace-1', 'view-1'), savedView())
  const view = renderHook(() => useSetFavorite('workspace-1'), { wrapper: withClient(client) })

  act(() => view.result.current.mutate({ viewId: 'view-1', favorite: true }))
  await waitFor(() => expect(client.getQueryData<SavedView[]>(queryKeys.views('workspace-1'))?.[0]?.is_favorite).toBeTrue())
  // the open view's header star too
  expect(client.getQueryData<SavedView>(queryKeys.view('workspace-1', 'view-1'))?.is_favorite).toBeTrue()
  await waitFor(() => expect(requests).toHaveLength(1))
  release?.()
  await waitFor(() => expect(view.result.current.isSuccess).toBeTrue())

  act(() => view.result.current.mutate({ viewId: 'view-1', favorite: false }))
  await waitFor(() => expect(requests).toHaveLength(2))
  release?.()
  expect(requests.map(({ method, path }) => [method, path])).toEqual([
    ['PUT', '/api/v1/workspaces/workspace-1/views/view-1/favorite'],
    ['DELETE', '/api/v1/workspaces/workspace-1/views/view-1/favorite'],
  ])
})

test('starring a view puts it after the existing favorites at once', async () => {
  serve(() => new Response(null, { status: 204 }))
  const client = testClient()
  client.setQueryData(queryKeys.views('workspace-1'), [
    savedView({ id: 'view-1' }),
    savedView({ id: 'view-2', is_favorite: true, favorite_position: 0 }),
    savedView({ id: 'view-3', is_favorite: true, favorite_position: 4 }),
  ])
  client.setQueryData(queryKeys.view('workspace-1', 'view-1'), savedView({ id: 'view-1' }))
  const view = renderHook(() => useSetFavorite('workspace-1'), { wrapper: withClient(client) })

  act(() => view.result.current.mutate({ viewId: 'view-1', favorite: true }))

  await waitFor(() => expect(client.getQueryData<SavedView[]>(queryKeys.views('workspace-1'))?.[0]).toMatchObject({ is_favorite: true, favorite_position: 5 }))
  expect(client.getQueryData<SavedView>(queryKeys.view('workspace-1', 'view-1'))).toMatchObject({ is_favorite: true, favorite_position: 5 })
})

test('a failed favorite toggle restores the star', async () => {
  serve(() => problem(500, 'internal_error'))
  const client = testClient()
  client.setQueryData(queryKeys.views('workspace-1'), [savedView()])
  client.setQueryData(queryKeys.view('workspace-1', 'view-1'), savedView())
  const view = renderHook(() => useSetFavorite('workspace-1'), { wrapper: withClient(client) })

  await act(async () => { await view.result.current.mutateAsync({ viewId: 'view-1', favorite: true }).catch(() => undefined) })

  expect(client.getQueryData<SavedView[]>(queryKeys.views('workspace-1'))?.[0]?.is_favorite).toBeFalse()
  expect(client.getQueryData<SavedView>(queryKeys.view('workspace-1', 'view-1'))?.is_favorite).toBeFalse()
})

test('reordering favorites sends the full order and updates positions at once', async () => {
  const requests = serve(() => new Response(null, { status: 204 }))
  const client = testClient()
  client.setQueryData(queryKeys.views('workspace-1'), [
    savedView({ id: 'view-1', is_favorite: true, favorite_position: 0 }),
    savedView({ id: 'view-2', is_favorite: true, favorite_position: 1 }),
  ])
  const view = renderHook(() => useReorderFavorites('workspace-1'), { wrapper: withClient(client) })

  act(() => view.result.current.mutate(['view-2', 'view-1']))

  await waitFor(() => expect(client.getQueryData<SavedView[]>(queryKeys.views('workspace-1'))?.map((item) => [item.id, item.favorite_position])).toEqual([['view-1', 1], ['view-2', 0]]))
  await waitFor(() => expect(requests).toEqual([{ method: 'PUT', path: '/api/v1/workspaces/workspace-1/view-favorites/order', body: { view_ids: ['view-2', 'view-1'] } }]))
})

test('a 404 preference resolves to null without retrying', async () => {
  const requests = serve(() => problem(404, 'task_resource_not_found'))
  const view = renderHook(() => useViewPreference('workspace-1', 'all'), { wrapper: withClient(new QueryClient()) })
  await waitFor(() => expect(view.result.current.isSuccess).toBeTrue())
  expect(view.result.current.data).toBeNull()
  expect(requests).toHaveLength(1)
})

test('a persistent preference error retries a bounded number of times before failing', async () => {
  const requests = serve(() => problem(500, 'internal_error'))
  const client = new QueryClient({ defaultOptions: { queries: { retryDelay: 0 } } })
  const view = renderHook(() => useViewPreference('workspace-1', 'all'), { wrapper: withClient(client) })
  await waitFor(() => expect(view.result.current.isError).toBeTrue())
  expect(requests.length).toBeGreaterThan(1)
  expect(requests.length).toBeLessThanOrEqual(4)
})
