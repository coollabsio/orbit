import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { Page, PageSummary, PageVersionSummary } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { usePageVersion, usePageVersions, useRestorePageVersion, VERSION_PAGE_SIZE } from './pageVersions'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

type Call = { method: string; path: string; search: URLSearchParams; body: unknown }

function mockFetch(handler: (call: Call) => Response) {
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    const text = await request.text()
    const call = { method: request.method, path: url.pathname, search: url.searchParams, body: text ? JSON.parse(text) : undefined }
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

const BASE = '/api/v1/workspaces/workspace-1/pages/page-1/versions'

const version = (id: string, created_at = '2026-09-25T10:00:00Z'): PageVersionSummary => ({
  id, page_id: 'page-1', kind: 'auto', title: 'Plan', icon: null, created_at, created_by: { id: 'user-1', display_name: 'Ada' },
})

const page = (patch: Partial<Page> = {}): Page => ({
  id: 'page-1', workspace_id: 'workspace-1', parent_id: null, teamspace_id: 'teamspace-1', private: false, title: 'Plan',
  icon: null, cover_url: null, cover_position: null, content: [], position: 0, version: 4, creator_id: 'user-1',
  updated_by: 'user-1', created_at: '2026-09-25T10:00:00Z', updated_at: '2026-09-25T10:00:00Z', deleted_at: null, collab_epoch: 'epoch-1',
  ...patch,
})

test('the version list follows next_cursor and flattens the pages, newest first', async () => {
  const calls = mockFetch((call) =>
    call.search.get('cursor') === 'b'
      ? Response.json({ items: [version('c')], next_cursor: null })
      : Response.json({ items: [version('a'), version('b')], next_cursor: 'b' }),
  )
  const { wrapper } = setupClient()
  const { result } = renderHook(() => usePageVersions('workspace-1', 'page-1'), { wrapper })
  await waitFor(() => expect(result.current.data?.map((item) => item.id)).toEqual(['a', 'b']))
  expect(result.current.hasNextPage).toBeTrue()
  expect(calls[0].path).toBe(BASE)
  expect(calls[0].search.get('limit')).toBe(String(VERSION_PAGE_SIZE))
  expect(calls[0].search.has('cursor')).toBeFalse()

  await act(async () => {
    await result.current.fetchNextPage()
  })
  await waitFor(() => expect(result.current.data?.map((item) => item.id)).toEqual(['a', 'b', 'c']))
  expect(result.current.hasNextPage).toBeFalse()
})

test('a version is fetched with its content only when one is selected', async () => {
  const calls = mockFetch(() => Response.json({ ...version('a'), content: [{ type: 'paragraph' }] }))
  const { wrapper } = setupClient()
  const { result, rerender } = renderHook(({ id }: { id: string | null }) => usePageVersion('workspace-1', 'page-1', id), {
    wrapper,
    initialProps: { id: null as string | null },
  })
  await new Promise((resolve) => setTimeout(resolve, 10))
  expect(calls).toHaveLength(0)
  rerender({ id: 'a' })
  await waitFor(() => expect(result.current.data?.content).toEqual([{ type: 'paragraph' }]))
  expect(calls[0].path).toBe(`${BASE}/a`)
})

test('restoring posts expected_version, stores the restored page and refetches the history', async () => {
  const restored = page({ title: 'Old plan', version: 5 })
  const calls = mockFetch((call) => (call.method === 'POST' ? Response.json(restored) : Response.json({ items: [], next_cursor: null })))
  const { client, wrapper } = setupClient()
  client.setQueryData(queryKeys.pages.tree('workspace-1'), [{ ...page(), content: undefined } as unknown as PageSummary])
  const versionsKey = queryKeys.pages.versions('workspace-1', 'page-1')
  client.setQueryData(versionsKey, { pages: [{ items: [version('a')], next_cursor: null }], pageParams: [undefined] })
  const { result } = renderHook(() => useRestorePageVersion('workspace-1', 'page-1'), { wrapper })
  const saved: Page[] = []
  await act(async () => {
    saved.push(await result.current.mutateAsync({ versionId: 'a', expectedVersion: 4 }))
  })
  expect(saved).toEqual([restored])
  const post = calls.find((call) => call.method === 'POST')!
  expect(post.path).toBe(`${BASE}/a/restore`)
  expect(post.body).toEqual({ expected_version: 4 })
  expect(client.getQueryData<Page>(queryKeys.pages.detail('workspace-1', 'page-1'))).toEqual(restored)
  expect(client.getQueryData<PageSummary[]>(queryKeys.pages.tree('workspace-1'))?.[0].title).toBe('Old plan')
  expect(client.getQueryState(versionsKey)?.isInvalidated).toBeTrue()
})
