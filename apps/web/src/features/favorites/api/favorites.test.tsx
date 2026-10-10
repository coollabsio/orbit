import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { queryKeys } from '@/api/queryKeys'
import { defaultViewState } from '@/features/views/viewState'
import type { SavedView } from '@/features/views/api/views'
import { useReorderFavorites, useSetFavorite, withViewOrder, type Favorite } from './favorites'

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
  is_favorite: false, favorite_position: null, can_edit: true, can_change_visibility: true, created_at: '2026-09-01T12:00:00Z', updated_at: '2026-09-01T12:00:00Z',
  ...overrides,
})

const testClient = () => new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
function withClient(client: QueryClient) {
  return ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
}

test('favoriting flips the star at once and uses PUT or DELETE', async () => {
  let release: (() => void) | undefined
  const requests = serve(() => new Promise<Response>((resolve) => { release = () => resolve(new Response(null, { status: 204 })) }))
  const client = testClient()
  client.setQueryData(queryKeys.views('workspace-1'), [savedView()])
  client.setQueryData(queryKeys.view('workspace-1', 'view-1'), savedView())
  const view = renderHook(() => useSetFavorite('workspace-1'), { wrapper: withClient(client) })

  act(() => view.result.current.mutate({ kind: 'view', targetId: 'view-1', favorite: true }))
  await waitFor(() => expect(client.getQueryData<SavedView[]>(queryKeys.views('workspace-1'))?.[0]?.is_favorite).toBeTrue())
  // the open view's header star too
  expect(client.getQueryData<SavedView>(queryKeys.view('workspace-1', 'view-1'))?.is_favorite).toBeTrue()
  await waitFor(() => expect(requests.filter(({ method }) => method !== 'GET')).toHaveLength(1))
  release?.()
  await waitFor(() => expect(view.result.current.isSuccess).toBeTrue())

  act(() => view.result.current.mutate({ kind: 'view', targetId: 'view-1', favorite: false }))
  await waitFor(() => expect(requests.filter(({ method }) => method !== 'GET')).toHaveLength(2))
  release?.()
  expect(requests.filter(({ method }) => method !== 'GET').map(({ method, path }) => [method, path])).toEqual([
    ['PUT', '/api/v1/workspaces/workspace-1/favorites/view/view-1'],
    ['DELETE', '/api/v1/workspaces/workspace-1/favorites/view/view-1'],
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

  act(() => view.result.current.mutate({ kind: 'view', targetId: 'view-1', favorite: true }))

  await waitFor(() => expect(client.getQueryData<SavedView[]>(queryKeys.views('workspace-1'))?.[0]).toMatchObject({ is_favorite: true, favorite_position: 5 }))
  expect(client.getQueryData<SavedView>(queryKeys.view('workspace-1', 'view-1'))).toMatchObject({ is_favorite: true, favorite_position: 5 })
})

test('a failed favorite toggle restores the star', async () => {
  serve(() => problem(500, 'internal_error'))
  const client = testClient()
  client.setQueryData(queryKeys.views('workspace-1'), [savedView()])
  client.setQueryData(queryKeys.view('workspace-1', 'view-1'), savedView())
  const view = renderHook(() => useSetFavorite('workspace-1'), { wrapper: withClient(client) })

  await act(async () => { await view.result.current.mutateAsync({ kind: 'view', targetId: 'view-1', favorite: true }).catch(() => undefined) })

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

  act(() => view.result.current.mutate([{ kind: 'view', targetId: 'view-2' }, { kind: 'view', targetId: 'view-1' }]))

  await waitFor(() => expect(client.getQueryData<SavedView[]>(queryKeys.views('workspace-1'))?.map((item) => [item.id, item.favorite_position])).toEqual([['view-1', 1], ['view-2', 0]]))
  await waitFor(() => expect(requests.filter(({ method }) => method !== 'GET')).toEqual([{ method: 'PUT', path: '/api/v1/workspaces/workspace-1/favorites/order', body: { items: [{ kind: 'view', target_id: 'view-2' }, { kind: 'view', target_id: 'view-1' }] } }]))
})

const favorite = (kind: string, id: string, position: number): Favorite => ({
  kind, target_id: id, title: id, identifier: null, color: null, icon: null, path: `/${kind}s/${id}`, position,
})

test('removing a task favorite takes it out of the sidebar list at once and a failure puts it back', async () => {
  let release: (() => void) | undefined
  serve(() => new Promise<Response>((resolve) => { release = () => resolve(problem(500, 'internal_error')) }))
  const client = testClient()
  const items = [favorite('task', 'task-1', 0), favorite('view', 'view-1', 1)]
  client.setQueryData(queryKeys.favorites('workspace-1'), items)
  const hook = renderHook(() => useSetFavorite('workspace-1'), { wrapper: withClient(client) })

  act(() => hook.result.current.mutate({ kind: 'task', targetId: 'task-1', favorite: false }))
  await waitFor(() => expect(client.getQueryData<Favorite[]>(queryKeys.favorites('workspace-1'))?.map((item) => item.target_id)).toEqual(['view-1']))
  await waitFor(() => expect(release).toBeDefined())
  release?.()
  await waitFor(() => expect(hook.result.current.isError).toBeTrue())
  expect(client.getQueryData<Favorite[]>(queryKeys.favorites('workspace-1'))?.map((item) => item.target_id)).toEqual(['task-1', 'view-1'])
})

test('a new order of the views keeps the tasks among the favorites in their slots', () => {
  const items = [favorite('view', 'view-1', 0), favorite('task', 'task-1', 1), favorite('view', 'view-2', 2)]
  expect(withViewOrder(items, ['view-2', 'view-1'])).toEqual([
    { kind: 'view', targetId: 'view-2' },
    { kind: 'task', targetId: 'task-1' },
    { kind: 'view', targetId: 'view-1' },
  ])
  // the favorites list is not loaded yet: the views alone
  expect(withViewOrder([], ['view-2'])).toEqual([{ kind: 'view', targetId: 'view-2' }])
})
