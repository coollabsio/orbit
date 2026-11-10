import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { PageFavorite } from '@orbit/api/generated/types.gen'
import { createApiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { favoriteDropIndex, favoritesQueryOptions, moveFavoriteId, useMoveFavorite, useToggleFavorite } from './favorites'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const problem = (status: number, code: string) =>
  Response.json(
    { type: 'about:blank', title: 'Problem', status, code, detail: code, instance: '/pages', request_id: 'request-1' },
    { status, headers: { 'content-type': 'application/problem+json' } },
  )

type Call = { method: string; path: string; body: unknown }

function mockFetch(handler: (call: Call) => Response | Promise<Response>) {
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const text = await request.text()
    const call = { method: request.method, path: new URL(request.url).pathname, body: text ? JSON.parse(text) : undefined }
    calls.push(call)
    return handler(call)
  }) as unknown as typeof fetch
  return calls
}

function setupClient() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  return { client, wrapper }
}

const key = queryKeys.pages.favorites('workspace-1')
const base = '/api/v1/workspaces/workspace-1/pages'

const entry = (page_id: string, position: number, patch: Partial<PageFavorite> = {}): PageFavorite => ({
  page_id, position, title: page_id.toUpperCase(), icon: null, kind: 'page', database_id: null, database_title: null, ...patch,
})
const entries = (...ids: string[]) => ids.map((id, position) => entry(id, position))
const cachedIds = (client: QueryClient) => client.getQueryData<PageFavorite[]>(key)?.map((favorite) => favorite.page_id)

test('the favorites query reads the ordered favorites of the caller', async () => {
  let requested = ''
  const client = createApiClient({
    fetch: async (request) => {
      requested = request.url
      return Response.json({ items: [entry('b', 1), entry('a', 0, { database_id: 'db', database_title: 'Research' })] })
    },
  })
  expect(await favoritesQueryOptions('workspace-1', client).queryFn()).toEqual([
    entry('a', 0, { database_id: 'db', database_title: 'Research' }),
    entry('b', 1),
  ])
  expect(new URL(requested).pathname).toBe(`${base}/favorites`)
})

test('adding a favorite appends it optimistically and PUTs it; removing DELETEs it', async () => {
  let finish: ((response: Response) => void) | undefined
  const calls = mockFetch((call) => {
    if (call.method === 'PUT') return new Promise<Response>((resolve) => { finish = resolve })
    if (call.method === 'DELETE') return new Response(null, { status: 204 })
    return new Promise<Response>(() => {})
  })
  const { client, wrapper } = setupClient()
  client.setQueryData(key, entries('a'))
  const view = renderHook(() => useToggleFavorite('workspace-1'), { wrapper })

  act(() => view.result.current.mutate({ pageId: 'b', favorite: true, page: { title: 'Paper', icon: '📄', kind: 'page', database_id: 'db' } }))
  await waitFor(() => expect(calls.some((call) => call.method === 'PUT')).toBeTrue())
  expect(client.getQueryData<PageFavorite[]>(key)).toEqual([entry('a', 0), entry('b', 1, { title: 'Paper', icon: '📄', database_id: 'db' })])
  expect(calls.find((call) => call.method === 'PUT')?.path).toBe(`${base}/b/favorite`)
  await act(async () => {
    finish?.(Response.json(entry('b', 1)))
  })
  await waitFor(() => expect(view.result.current.isSuccess).toBeTrue())

  await act(async () => {
    await view.result.current.mutateAsync({ pageId: 'a', favorite: false })
  })
  expect(calls.find((call) => call.method === 'DELETE')?.path).toBe(`${base}/a/favorite`)
  expect(cachedIds(client)).toEqual(['b'])
})

test('a failed toggle rolls the favorites back', async () => {
  mockFetch((call) => (call.method === 'PUT' ? problem(404, 'page_not_found') : new Promise<Response>(() => {})))
  const { client, wrapper } = setupClient()
  client.setQueryData(key, entries('a'))
  const view = renderHook(() => useToggleFavorite('workspace-1'), { wrapper })
  await act(async () => {
    await view.result.current.mutateAsync({ pageId: 'b', favorite: true }).catch(() => undefined)
  })
  expect(cachedIds(client)).toEqual(['a'])
})

test('moving a favorite reorders optimistically, sends the index, and adopts the server order', async () => {
  let finish: ((response: Response) => void) | undefined
  const calls = mockFetch(() => new Promise<Response>((resolve) => { finish = resolve }))
  const { client, wrapper } = setupClient()
  client.setQueryData(key, entries('a', 'b', 'c'))
  const view = renderHook(() => useMoveFavorite('workspace-1'), { wrapper })

  act(() => view.result.current.mutate({ pageId: 'c', position: 0 }))
  await waitFor(() => expect(calls).toHaveLength(1))
  expect(calls[0]).toEqual({ method: 'POST', path: `${base}/favorites/c/move`, body: { position: 0 } })
  expect(client.getQueryData<PageFavorite[]>(key)).toEqual(entries('c', 'a', 'b'))
  await act(async () => {
    finish?.(Response.json({ items: entries('c', 'b', 'a') }))
  })
  await waitFor(() => expect(cachedIds(client)).toEqual(['c', 'b', 'a']))
})

test('a failed favorite move rolls back', async () => {
  mockFetch((call) => (call.method === 'POST' ? problem(422, 'validation_failed') : new Promise<Response>(() => {})))
  const { client, wrapper } = setupClient()
  client.setQueryData(key, entries('a', 'b'))
  const view = renderHook(() => useMoveFavorite('workspace-1'), { wrapper })
  await act(async () => {
    await view.result.current.mutateAsync({ pageId: 'b', position: 0 }).catch(() => undefined)
  })
  expect(cachedIds(client)).toEqual(['a', 'b'])
})

test('drop positions map to indexes among the favorites', () => {
  const ids = ['a', 'b', 'c', 'd']
  expect(favoriteDropIndex(ids, 'd', 'a', 'before')).toBe(0)
  expect(favoriteDropIndex(ids, 'a', 'c', 'after')).toBe(2)
  expect(favoriteDropIndex(ids, 'a', 'd', 'after')).toBe(3)
  expect(favoriteDropIndex(ids, 'b', 'c', 'before')).toBeNull()
  expect(favoriteDropIndex(ids, 'b', 'a', 'after')).toBeNull()
  expect(favoriteDropIndex(ids, 'b', 'b', 'before')).toBeNull()
  expect(favoriteDropIndex(ids, 'x', 'a', 'before')).toBeNull()
  expect(favoriteDropIndex(ids, 'a', 'x', 'before')).toBeNull()
  expect(moveFavoriteId(ids, 'a', 2)).toEqual(['b', 'c', 'a', 'd'])
  expect(moveFavoriteId(ids, 'a', 99)).toEqual(['b', 'c', 'd', 'a'])
  expect(moveFavoriteId(ids, 'x', 0)).toEqual(ids)
})
