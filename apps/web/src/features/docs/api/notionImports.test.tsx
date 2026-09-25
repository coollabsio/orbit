import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { NotionImport } from '@/api/generated/types.gen'
import { createApiClient } from '@/api/client'
import { ApiProblem } from '@/api/problem'
import { queryKeys } from '@/api/queryKeys'
import {
  createNotionImportRequest,
  isActiveImport,
  isFinalImport,
  notionImportErrorMessage,
  useCancelNotionImport,
  useNotionImport,
  useStartNotionImport,
} from './notionImports'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const notionImport = (patch: Partial<NotionImport> = {}): NotionImport => ({
  id: 'import-1', workspace_id: 'workspace-1', status: 'scanning', notion_workspace_name: 'Acme Notion', error: null,
  progress: { total: 0, done: 0, failed: 0 }, destination: null, tree: null, report: null, root_page_ids: [],
  created_at: '2026-09-25T10:00:00Z', updated_at: '2026-09-25T10:00:00Z', ...patch,
})

const problem = (status: number, code: string) =>
  new ApiProblem({ type: 'about:blank', title: 'Problem', status, code, detail: code, instance: '/imports', request_id: 'request-1' })

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
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  return { client, wrapper }
}

const base = '/api/v1/workspaces/workspace-1/imports/notion'

test('status helpers: active imports poll, final ones never change', () => {
  expect(['scanning', 'queued', 'importing'].every((status) => isActiveImport(status as NotionImport['status']))).toBe(true)
  expect(isActiveImport('ready')).toBe(false)
  expect(['completed', 'failed', 'cancelled', 'expired'].every((status) => isFinalImport(status as NotionImport['status']))).toBe(true)
  expect(isFinalImport('ready')).toBe(false)
})

test('problems map to friendly messages', () => {
  expect(notionImportErrorMessage(problem(422, 'notion_token_invalid'))).toContain('Notion rejected this token')
  expect(notionImportErrorMessage(problem(409, 'app_key_missing'))).toBe(
    'The server administrator must set an app key before anyone can import from Notion.',
  )
  expect(notionImportErrorMessage(problem(409, 'import_in_progress'))).toContain('already have an import running')
  expect(notionImportErrorMessage(problem(502, 'notion_unavailable'))).toContain('could not be reached')
  expect(notionImportErrorMessage(problem(422, 'notion_import_too_large'))).toContain('5,000')
  expect(notionImportErrorMessage(new Error('boom'))).toBe('Something went wrong. Try again.')
})

test('the token is posted once to the collection', async () => {
  const calls = mockFetch(() => Response.json(notionImport(), { status: 201 }))
  const created = await createNotionImportRequest(createApiClient(), 'workspace-1', 'ntn_secret')
  expect(created.id).toBe('import-1')
  expect(calls).toEqual([{ method: 'POST', path: base, body: { token: 'ntn_secret' } }])
})

test('an import is polled while scanning and stops at a final state, refreshing the page tree', async () => {
  let status: NotionImport['status'] = 'scanning'
  const calls = mockFetch(() => Response.json(notionImport({ status })))
  const polls = () => calls.filter((call) => call.path === `${base}/import-1`).length
  const { client, wrapper } = setupClient()
  client.setQueryData(queryKeys.pages.tree('workspace-1'), [])
  const view = renderHook(() => useNotionImport('workspace-1', 'import-1', 20), { wrapper })
  await waitFor(() => expect(view.result.current.data?.status).toBe('scanning'))
  await waitFor(() => expect(polls()).toBeGreaterThanOrEqual(3))

  // Scan done: "ready" waits for the member, so polling stops and nothing is refreshed.
  status = 'ready'
  await waitFor(() => expect(view.result.current.data?.status).toBe('ready'))
  const after = polls()
  await act(() => new Promise((resolve) => setTimeout(resolve, 80)))
  expect(polls()).toBe(after)
  expect(client.getQueryState(queryKeys.pages.tree('workspace-1'))?.isInvalidated).toBe(false)

  // Started → importing (polled again) → completed: the page queries are invalidated once the final state arrives.
  status = 'importing'
  act(() => client.setQueryData(queryKeys.notionImports.detail('workspace-1', 'import-1'), notionImport({ status: 'importing' })))
  await waitFor(() => expect(polls()).toBeGreaterThan(after))
  status = 'completed'
  await waitFor(() => expect(view.result.current.data?.status).toBe('completed'))
  await waitFor(() => expect(client.getQueryState(queryKeys.pages.tree('workspace-1'))?.isInvalidated).toBe(true))
  const final = polls()
  await act(() => new Promise((resolve) => setTimeout(resolve, 80)))
  expect(polls()).toBe(final)
})

test('start sends selection and destination and stores the queued import', async () => {
  const calls = mockFetch(() => Response.json(notionImport({ status: 'queued', progress: { total: 3, done: 0, failed: 0 } })))
  const { client, wrapper } = setupClient()
  const view = renderHook(() => useStartNotionImport('workspace-1'), { wrapper })
  await act(() =>
    view.result.current.mutateAsync({ importId: 'import-1', selection: { notion_ids: ['a'] }, destination: { private: true } }),
  )
  expect(calls[0]).toEqual({
    method: 'POST',
    path: `${base}/import-1/start`,
    body: { selection: { notion_ids: ['a'] }, destination: { private: true } },
  })
  expect(client.getQueryData<NotionImport>(queryKeys.notionImports.detail('workspace-1', 'import-1'))?.status).toBe('queued')
})

test('cancel stores the cancelled import and refreshes pages (empty pages went to the trash)', async () => {
  const calls = mockFetch(() => Response.json(notionImport({ status: 'cancelled' })))
  const { client, wrapper } = setupClient()
  client.setQueryData(queryKeys.pages.tree('workspace-1'), [])
  const view = renderHook(() => useCancelNotionImport('workspace-1'), { wrapper })
  await act(() => view.result.current.mutateAsync({ importId: 'import-1' }))
  expect(calls[0]).toMatchObject({ method: 'POST', path: `${base}/import-1/cancel` })
  expect(client.getQueryData<NotionImport>(queryKeys.notionImports.detail('workspace-1', 'import-1'))?.status).toBe('cancelled')
  expect(client.getQueryState(queryKeys.pages.tree('workspace-1'))?.isInvalidated).toBe(true)
})
