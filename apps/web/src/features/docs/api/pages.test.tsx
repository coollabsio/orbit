import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { createApiClient } from '@/api/client'
import type { Page, PageSummary } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { queryKeys } from '@/api/queryKeys'
import {
  canPurgePage,
  conflictCurrentPage,
  conflictCurrentVersion,
  isPageNotFound,
  isPageVersionConflict,
  pageTreeQueryOptions,
  pageUploadErrorMessage,
  uploadPageFileRequest,
  useCreatePage,
  useDuplicatePage,
  useEmptyPageTrash,
  useMovePage,
  usePage,
  usePageSearch,
  usePurgePage,
  useRestorePage,
  useTrashPage,
} from './pages'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const summary = (id: string, parent_id: string | null, position: number, version = 1): PageSummary => ({
  id, parent_id, teamspace_id: 'teamspace-1', private: false, position, version, title: id, icon: null,
  updated_at: '2026-09-25T10:00:00Z',
})

const fullPage = (id: string, parent_id: string | null, position: number, version = 1): Page => ({
  ...summary(id, parent_id, position, version),
  workspace_id: 'workspace-1', cover_url: null, cover_position: null, content: [], creator_id: 'user-1',
  updated_by: 'user-1', created_at: '2026-09-25T10:00:00Z', deleted_at: null, collab_epoch: 'epoch-1',
  full_width: false, locked_at: null, locked_by: null, updated_by_user: { id: 'user-1', display_name: 'Ada' },
})

const problem = (status: number, code: string, extra: Record<string, unknown> = {}) =>
  Response.json(
    { type: 'about:blank', title: 'Problem', status, code, detail: code, instance: '/pages', request_id: 'request-1', ...extra },
    { status, headers: { 'content-type': 'application/problem+json' } },
  )

type Call = { method: string; path: string; search: string; body: unknown }

function mockFetch(handler: (call: Call) => Response | Promise<Response>) {
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    const text = await request.text()
    const call = { method: request.method, path: url.pathname, search: url.search, body: text ? JSON.parse(text) : undefined }
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

const treeKey = queryKeys.pages.tree('workspace-1')

test('the tree query reads every live page from the pages endpoint', async () => {
  let requested = ''
  const client = createApiClient({
    fetch: async (request) => {
      requested = request.url
      return Response.json({ items: [summary('a', null, 0), summary('b', 'a', 0)] })
    },
  })
  const pages = await pageTreeQueryOptions('workspace-1', client).queryFn()
  expect(new URL(requested).pathname).toBe('/api/v1/workspaces/workspace-1/pages')
  expect(pages.map((page) => page.id)).toEqual(['a', 'b'])
})

test('a trashed or unknown page reports not found', async () => {
  mockFetch(() => problem(404, 'page_not_found'))
  const { wrapper } = setupClient()
  const view = renderHook(() => usePage('workspace-1', 'gone'), { wrapper })
  await waitFor(() => expect(view.result.current.isError).toBeTrue())
  expect(isPageNotFound(view.result.current.error)).toBeTrue()
})

test('creating a page adds it to the tree cache and seeds its detail query', async () => {
  const calls = mockFetch((call) =>
    call.method === 'POST' ? Response.json(fullPage('child', 'a', 1), { status: 201 }) : Response.json({ items: [summary('a', null, 0), summary('child', 'a', 1)] }),
  )
  const { client, wrapper } = setupClient()
  client.setQueryData(treeKey, [summary('a', null, 0)])
  const view = renderHook(() => useCreatePage('workspace-1'), { wrapper })

  await act(async () => {
    await view.result.current.mutateAsync({ parent_id: 'a' })
  })
  expect(calls[0]).toMatchObject({ method: 'POST', path: '/api/v1/workspaces/workspace-1/pages', body: { parent_id: 'a' } })
  expect(client.getQueryData<PageSummary[]>(treeKey)?.map((page) => page.id)).toContain('child')
  expect(client.getQueryData<Page>(queryKeys.pages.detail('workspace-1', 'child'))?.parent_id).toBe('a')
})

test('moving a page updates the tree optimistically and sends parent, index and version', async () => {
  let finish: ((response: Response) => void) | undefined
  const calls = mockFetch((call) =>
    call.path.endsWith('/move')
      ? new Promise<Response>((resolve) => { finish = resolve })
      : Response.json({ items: [] }),
  )
  const { client, wrapper } = setupClient()
  client.setQueryData(treeKey, [summary('a', null, 0), summary('b', null, 1, 4)])
  const view = renderHook(() => useMovePage('workspace-1'), { wrapper })

  act(() => view.result.current.mutate({ pageId: 'b', version: 4, parent_id: 'a', position: 0 }))
  await waitFor(() => expect(calls.some((call) => call.path.endsWith('/move'))).toBeTrue())
  const optimistic = client.getQueryData<PageSummary[]>(treeKey)!
  expect(optimistic.find((page) => page.id === 'b')).toMatchObject({ parent_id: 'a', position: 0 })
  expect(calls.find((call) => call.path.endsWith('/move'))).toMatchObject({
    method: 'POST',
    path: '/api/v1/workspaces/workspace-1/pages/b/move',
    body: { expected_version: 4, parent_id: 'a', position: 0 },
  })

  await act(async () => {
    finish?.(Response.json(fullPage('b', 'a', 0, 5)))
  })
  await waitFor(() => expect(view.result.current.isSuccess).toBeTrue())
  expect(client.getQueryData<Page>(queryKeys.pages.detail('workspace-1', 'b'))?.version).toBe(5)
})

test('moving to another space sends the space and moves the subtree optimistically', async () => {
  const calls = mockFetch((call) => (call.path.endsWith('/move') ? new Promise<Response>(() => {}) : Response.json({ items: [] })))
  const { client, wrapper } = setupClient()
  const mine = (id: string, parent_id: string | null, position: number): PageSummary => ({ ...summary(id, parent_id, position), teamspace_id: null, private: true })
  client.setQueryData(treeKey, [summary('g', null, 0), mine('p', null, 0), mine('child', 'p', 0), mine('grandchild', 'child', 0)])
  const view = renderHook(() => useMovePage('workspace-1'), { wrapper })

  act(() => view.result.current.mutate({ pageId: 'p', version: 1, parent_id: null, position: 1, teamspace_id: 'teamspace-1' }))
  await waitFor(() => expect(calls.some((call) => call.path.endsWith('/move'))).toBeTrue())
  expect(calls.find((call) => call.path.endsWith('/move'))?.body).toEqual({
    expected_version: 1, parent_id: null, position: 1, teamspace_id: 'teamspace-1',
  })
  const tree = client.getQueryData<PageSummary[]>(treeKey)!
  for (const id of ['p', 'child', 'grandchild']) {
    expect(tree.find((page) => page.id === id)).toMatchObject({ teamspace_id: 'teamspace-1', private: false })
  }
  expect(tree.find((page) => page.id === 'p')).toMatchObject({ parent_id: null, position: 1 })

  act(() => view.result.current.mutate({ pageId: 'g', version: 1, parent_id: null, position: 0, private: true }))
  await waitFor(() => expect(calls.filter((call) => call.path.endsWith('/move'))).toHaveLength(2))
  expect(calls.filter((call) => call.path.endsWith('/move'))[1].body).toEqual({ expected_version: 1, parent_id: null, position: 0, private: true })
})

test('a failed move rolls the tree back', async () => {
  mockFetch((call) => (call.path.endsWith('/move') ? problem(422, 'validation_failed') : new Promise<Response>(() => {})))
  const { client, wrapper } = setupClient()
  const before = [summary('a', null, 0), summary('b', null, 1)]
  client.setQueryData(treeKey, before)
  const view = renderHook(() => useMovePage('workspace-1'), { wrapper })

  await act(async () => {
    await view.result.current.mutateAsync({ pageId: 'a', version: 1, parent_id: 'b', position: 0 }).catch(() => undefined)
  })
  expect(client.getQueryData<PageSummary[]>(treeKey)).toEqual(before)
})

test('trashing removes the subtree optimistically, sends expected_version, and rolls back on failure', async () => {
  let fail = false
  const calls = mockFetch((call) => {
    if (call.method === 'DELETE') return fail ? problem(500, 'internal_error') : new Response(null, { status: 204 })
    return new Promise<Response>(() => {})
  })
  const { client, wrapper } = setupClient()
  const before = [summary('a', null, 0), summary('b', 'a', 0), summary('c', null, 1)]
  client.setQueryData(treeKey, before)
  client.setQueryData(queryKeys.pages.detail('workspace-1', 'b'), fullPage('b', 'a', 0))
  const view = renderHook(() => useTrashPage('workspace-1'), { wrapper })

  await act(async () => {
    await view.result.current.mutateAsync({ pageId: 'a', version: 3 })
  })
  const deleteCall = calls.find((call) => call.method === 'DELETE')!
  expect(deleteCall.path).toBe('/api/v1/workspaces/workspace-1/pages/a')
  expect(new URLSearchParams(deleteCall.search).get('expected_version')).toBe('3')
  expect(client.getQueryData<PageSummary[]>(treeKey)?.map((page) => page.id)).toEqual(['c'])
  expect(client.getQueryData(queryKeys.pages.detail('workspace-1', 'b'))).toBeUndefined()

  fail = true
  client.setQueryData(treeKey, before)
  await act(async () => {
    await view.result.current.mutateAsync({ pageId: 'c', version: 1 }).catch(() => undefined)
  })
  expect(client.getQueryData<PageSummary[]>(treeKey)).toEqual(before)
})

test('restoring posts the trashed version and refreshes tree and trash', async () => {
  const calls = mockFetch((call) =>
    call.path.endsWith('/restore') ? Response.json(fullPage('a', null, 0, 7)) : Response.json({ items: [] }),
  )
  const { client, wrapper } = setupClient()
  client.setQueryData(treeKey, [])
  client.setQueryData(queryKeys.pages.trash('workspace-1'), [])
  const view = renderHook(() => useRestorePage('workspace-1'), { wrapper })

  await act(async () => {
    await view.result.current.mutateAsync({ pageId: 'a', version: 6 })
  })
  expect(calls[0]).toMatchObject({ method: 'POST', path: '/api/v1/workspaces/workspace-1/pages/a/restore', body: { expected_version: 6 } })
  expect(client.getQueryState(treeKey)?.isInvalidated).toBeTrue()
  expect(client.getQueryState(queryKeys.pages.trash('workspace-1'))?.isInvalidated).toBeTrue()
})

test('page search stays idle for a blank query and searches trimmed text', async () => {
  const calls = mockFetch(() => Response.json({ items: [{ id: 'a', parent_id: null, title: 'Roadmap', icon: null, snippet: 'Q3 plan' }] }))
  const { wrapper } = setupClient()
  const view = renderHook(({ q }) => usePageSearch('workspace-1', q), { initialProps: { q: '   ' }, wrapper })
  await new Promise((resolve) => setTimeout(resolve, 10))
  expect(calls).toHaveLength(0)
  expect(view.result.current.fetchStatus).toBe('idle')

  view.rerender({ q: ' plan ' })
  await waitFor(() => expect(view.result.current.data?.[0]?.title).toBe('Roadmap'))
  expect(calls[0].path).toBe('/api/v1/workspaces/workspace-1/pages/search')
  expect(new URLSearchParams(calls[0].search).get('q')).toBe('plan')
})

test('409 conflicts expose the current version and page', () => {
  const current = fullPage('a', null, 0, 8)
  const conflict = new ApiProblem({
    type: 'about:blank', title: 'Conflict', status: 409, code: 'conflict', detail: 'stale', instance: '/pages/a', request_id: 'r',
    conflict: { current_version: 8, refresh: '/pages/a', current } as never,
  })
  const other = new ApiProblem({ type: 'about:blank', title: 'Invalid', status: 422, code: 'validation_failed', detail: 'x', instance: '/pages/a', request_id: 'r' })
  expect(isPageVersionConflict(conflict)).toBeTrue()
  expect(isPageVersionConflict(other)).toBeFalse()
  expect(conflictCurrentVersion(conflict)).toBe(8)
  expect(conflictCurrentPage(conflict)?.version).toBe(8)
  expect(conflictCurrentPage(other)).toBeNull()
})

test('page uploads post one multipart file field to the page files endpoint and return its URL', async () => {
  let seen: { method: string; path: string; type: string | null; file: FormDataEntryValue | null } | undefined
  const file = { id: 'file-1', page_id: 'page-1', url: '/api/v1/workspaces/workspace-1/pages/page-1/files/file-1', file_name: 'a.png', mime_type: 'image/png', size_bytes: 4, created_at: '2026-09-25T10:00:00Z' }
  const client = createApiClient({
    fetch: async (request) => {
      const form = await request.formData()
      seen = { method: request.method, path: new URL(request.url).pathname, type: request.headers.get('content-type'), file: form.get('file') }
      return Response.json(file, { status: 201 })
    },
  })
  const uploaded = await uploadPageFileRequest(client, 'workspace-1', 'page-1', new File(['png!'], 'a.png', { type: 'image/png' }))
  expect(uploaded.url).toBe(file.url)
  expect(seen?.method).toBe('POST')
  expect(seen?.path).toBe('/api/v1/workspaces/workspace-1/pages/page-1/files')
  expect(seen?.type).toStartWith('multipart/form-data; boundary=')
  expect((seen!.file as File).name).toBe('a.png')
})

test('upload failures map to short messages', async () => {
  const client = createApiClient({ fetch: async () => problem(413, 'upload_too_large') })
  const error = await uploadPageFileRequest(client, 'workspace-1', 'page-1', new File(['x'], 'x.bin')).catch((caught: unknown) => caught)
  expect(pageUploadErrorMessage(error)).toBe('The file is too large to upload.')
  expect(pageUploadErrorMessage(new ApiProblem({ type: 'x', title: 'x', status: 404, code: 'page_file_not_found', detail: 'x', instance: '/', request_id: 'r' }))).toBe(
    'This page is no longer available.',
  )
  expect(pageUploadErrorMessage(new Error('offline'))).toBe('Could not upload the file. Try again.')
})

test('only own private pages, or any page for owners and admins, can be deleted forever', () => {
  expect(canPurgePage({ private: true }, 'member')).toBeTrue()
  expect(canPurgePage({ private: false }, 'member')).toBeFalse()
  expect(canPurgePage({ private: false }, undefined)).toBeFalse()
  expect(canPurgePage({ private: false }, 'admin')).toBeTrue()
  expect(canPurgePage({ private: false }, 'owner')).toBeTrue()
})

test('deleting forever sends expected_version to /permanent and drops the row from the trash cache', async () => {
  const calls = mockFetch((call) => (call.method === 'DELETE' ? new Response(null, { status: 204 }) : new Promise<Response>(() => {})))
  const { client, wrapper } = setupClient()
  const trashKey = queryKeys.pages.trash('workspace-1')
  const row = (id: string) => ({ ...summary(id, null, 0), deleted_at: '2026-09-25T10:00:00Z' })
  client.setQueryData(trashKey, [row('a'), row('b')])
  const view = renderHook(() => usePurgePage('workspace-1'), { wrapper })

  await act(async () => {
    await view.result.current.mutateAsync({ pageId: 'a', version: 4 })
  })
  expect(calls[0].path).toBe('/api/v1/workspaces/workspace-1/pages/a/permanent')
  expect(new URLSearchParams(calls[0].search).get('expected_version')).toBe('4')
  expect(client.getQueryData<{ id: string }[]>(trashKey)?.map((page) => page.id)).toEqual(['b'])
})

test('emptying the trash posts once and refreshes the trash', async () => {
  const calls = mockFetch((call) => (call.method === 'POST' ? Response.json({ purged: 3 }) : new Promise<Response>(() => {})))
  const { client, wrapper } = setupClient()
  client.setQueryData(queryKeys.pages.trash('workspace-1'), [])
  const view = renderHook(() => useEmptyPageTrash('workspace-1'), { wrapper })
  let result: { purged: number } | undefined
  await act(async () => {
    result = await view.result.current.mutateAsync()
  })
  expect(result).toEqual({ purged: 3 })
  expect(calls[0]).toMatchObject({ method: 'POST', path: '/api/v1/workspaces/workspace-1/pages/trash/empty' })
  expect(client.getQueryState(queryKeys.pages.trash('workspace-1'))?.isInvalidated).toBeTrue()
})

test('duplicating posts include_children, seeds the copy and refreshes the tree', async () => {
  const calls = mockFetch((call) =>
    call.method === 'POST' ? Response.json(fullPage('copy', null, 1), { status: 201 }) : new Promise<Response>(() => {}),
  )
  const { client, wrapper } = setupClient()
  client.setQueryData(treeKey, [summary('a', null, 0)])
  const view = renderHook(() => useDuplicatePage('workspace-1'), { wrapper })
  await act(async () => {
    await view.result.current.mutateAsync({ pageId: 'a', includeChildren: true })
  })
  expect(calls[0]).toMatchObject({ method: 'POST', path: '/api/v1/workspaces/workspace-1/pages/a/duplicate', body: { include_children: true } })
  expect(client.getQueryData<Page>(queryKeys.pages.detail('workspace-1', 'copy'))?.id).toBe('copy')
  expect(client.getQueryState(treeKey)?.isInvalidated).toBeTrue()
})
