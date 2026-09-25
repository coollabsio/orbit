import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
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

test('the favorites query reads the ordered page ids of the caller', async () => {
  let requested = ''
  const client = createApiClient({
    fetch: async (request) => {
      requested = request.url
      return Response.json({ items: [{ page_id: 'b', position: 1 }, { page_id: 'a', position: 0 }] })
    },
  })
  expect(await favoritesQueryOptions('workspace-1', client).queryFn()).toEqual(['a', 'b'])
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
  client.setQueryData(key, ['a'])
  const view = renderHook(() => useToggleFavorite('workspace-1'), { wrapper })

  act(() => view.result.current.mutate({ pageId: 'b', favorite: true }))
  await waitFor(() => expect(calls.some((call) => call.method === 'PUT')).toBeTrue())
  expect(client.getQueryData<string[]>(key)).toEqual(['a', 'b'])
  expect(calls.find((call) => call.method === 'PUT')?.path).toBe(`${base}/b/favorite`)
  await act(async () => {
    finish?.(Response.json({ page_id: 'b', position: 1 }))
  })
  await waitFor(() => expect(view.result.current.isSuccess).toBeTrue())

  await act(async () => {
    await view.result.current.mutateAsync({ pageId: 'a', favorite: false })
  })
  expect(calls.find((call) => call.method === 'DELETE')?.path).toBe(`${base}/a/favorite`)
  expect(client.getQueryData<string[]>(key)).toEqual(['b'])
})

test('a failed toggle rolls the favorites back', async () => {
  mockFetch((call) => (call.method === 'PUT' ? problem(404, 'page_not_found') : new Promise<Response>(() => {})))
  const { client, wrapper } = setupClient()
  client.setQueryData(key, ['a'])
  const view = renderHook(() => useToggleFavorite('workspace-1'), { wrapper })
  await act(async () => {
    await view.result.current.mutateAsync({ pageId: 'b', favorite: true }).catch(() => undefined)
  })
  expect(client.getQueryData<string[]>(key)).toEqual(['a'])
})

test('moving a favorite reorders optimistically, sends the index, and adopts the server order', async () => {
  let finish: ((response: Response) => void) | undefined
  const calls = mockFetch(() => new Promise<Response>((resolve) => { finish = resolve }))
  const { client, wrapper } = setupClient()
  client.setQueryData(key, ['a', 'b', 'c'])
  const view = renderHook(() => useMoveFavorite('workspace-1'), { wrapper })

  act(() => view.result.current.mutate({ pageId: 'c', position: 0 }))
  await waitFor(() => expect(calls).toHaveLength(1))
  expect(calls[0]).toEqual({ method: 'POST', path: `${base}/favorites/c/move`, body: { position: 0 } })
  expect(client.getQueryData<string[]>(key)).toEqual(['c', 'a', 'b'])
  await act(async () => {
    finish?.(Response.json({ items: [{ page_id: 'c', position: 0 }, { page_id: 'b', position: 1 }, { page_id: 'a', position: 2 }] }))
  })
  await waitFor(() => expect(client.getQueryData<string[]>(key)).toEqual(['c', 'b', 'a']))
})

test('a failed favorite move rolls back', async () => {
  mockFetch((call) => (call.method === 'POST' ? problem(422, 'validation_failed') : new Promise<Response>(() => {})))
  const { client, wrapper } = setupClient()
  client.setQueryData(key, ['a', 'b'])
  const view = renderHook(() => useMoveFavorite('workspace-1'), { wrapper })
  await act(async () => {
    await view.result.current.mutateAsync({ pageId: 'b', position: 0 }).catch(() => undefined)
  })
  expect(client.getQueryData<string[]>(key)).toEqual(['a', 'b'])
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
