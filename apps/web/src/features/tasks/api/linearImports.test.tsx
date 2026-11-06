import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { LinearImport, LinearImportTree } from '@orbit/api/generated/types.gen'
import { createApiClient } from '@/api/client'
import { ApiProblem } from '@orbit/api/problem'
import { queryKeys } from '@/api/queryKeys'
import {
  linearImportErrorMessage,
  createLinearImportRequest,
  useLinearImport,
  useLinearImportTree,
  useCancelLinearImport,
  useStartLinearImport,
} from './linearImports'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const linearImport = (patch: Partial<LinearImport> = {}): LinearImport => ({
  id: 'import-1', workspace_id: 'workspace-1', status: 'scanning', linear_user_name: 'Ada', error: null,
  progress: { total: 0, done: 0, failed: 0 }, tree: null, report: null, project_ids: [],
  created_at: '2026-11-04T10:00:00Z', updated_at: '2026-11-04T10:00:00Z', ...patch,
})

const problem = (status: number, code: string) =>
  new ApiProblem({ type: 'about:blank', title: 'Problem', status, code, detail: code, instance: '/imports', request_id: 'request-1' })

type Call = { method: string; path: string; search: string; body: unknown }

function mockFetch(handler: (call: Call) => Response | Promise<Response>) {
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const text = await request.text()
    const url = new URL(request.url)
    const call = { method: request.method, path: url.pathname, search: url.search, body: text ? JSON.parse(text) : undefined }
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

const base = '/api/v1/workspaces/workspace-1/imports/linear'

test('problems map to friendly messages', () => {
  expect(linearImportErrorMessage(problem(422, 'linear_token_invalid'))).toBe(
    'Linear rejected this API key. Check that you copied the whole key and that it was not revoked.',
  )
  expect(linearImportErrorMessage(problem(502, 'linear_unavailable'))).toBe('Linear could not be reached. Try again in a moment.')
  expect(linearImportErrorMessage(problem(409, 'import_in_progress'))).toBe('You already have an import running. Wait for it to finish or cancel it.')
  expect(linearImportErrorMessage(problem(422, 'validation_failed'))).toBe('The request was not valid. Check your input and try again.')
  expect(linearImportErrorMessage(new Error('boom'))).toBe('Something went wrong. Try again.')
})

test('the API key is posted once to the collection', async () => {
  const calls = mockFetch(() => Response.json(linearImport(), { status: 201 }))
  const created = await createLinearImportRequest(createApiClient(), 'workspace-1', 'lin_api_secret')
  expect(created.id).toBe('import-1')
  expect(calls).toEqual([{ method: 'POST', path: base, search: '', body: { token: 'lin_api_secret' } }])
})

test('an import is polled while it works and refreshes projects and tasks when it ends', async () => {
  let status: LinearImport['status'] = 'importing'
  const calls = mockFetch(() => Response.json(linearImport({ status })))
  const polls = () => calls.filter((call) => call.path === `${base}/import-1`).length
  const { client, wrapper } = setupClient()
  client.setQueryData(queryKeys.projects('workspace-1'), [])
  client.setQueryData(queryKeys.tasks.list('workspace-1'), [])
  const view = renderHook(() => useLinearImport('workspace-1', 'import-1', 20), { wrapper })
  await waitFor(() => expect(view.result.current.data?.status).toBe('importing'))
  await waitFor(() => expect(polls()).toBeGreaterThanOrEqual(3))
  expect(client.getQueryState(queryKeys.projects('workspace-1'))?.isInvalidated).toBe(false)

  status = 'completed'
  await waitFor(() => expect(view.result.current.data?.status).toBe('completed'))
  await waitFor(() => expect(client.getQueryState(queryKeys.projects('workspace-1'))?.isInvalidated).toBe(true))
  expect(client.getQueryState(queryKeys.tasks.list('workspace-1'))?.isInvalidated).toBe(true)
  const final = polls()
  await act(() => new Promise((resolve) => setTimeout(resolve, 80)))
  expect(polls()).toBe(final)
  expect(calls.every((call) => call.search === '')).toBe(true)
})

test('the scan tree is fetched once with ?include=tree under its own key', async () => {
  const tree: LinearImportTree = {
    organization: 'Acme',
    teams: [{ id: 'team-1', key: 'ENG', name: 'Engineering', issue_count: 12, archived: false }],
    truncated: false,
  }
  const calls = mockFetch((call) => Response.json(linearImport({ status: 'ready', tree: call.search === '?include=tree' ? tree : null })))
  const { client, wrapper } = setupClient()
  const view = renderHook(() => useLinearImportTree('workspace-1', 'import-1'), { wrapper })
  await waitFor(() => expect(view.result.current.data).toEqual(tree))
  await act(() => new Promise((resolve) => setTimeout(resolve, 80)))
  expect(calls).toEqual([{ method: 'GET', path: `${base}/import-1`, search: '?include=tree', body: undefined }])
  expect(client.getQueryData(queryKeys.linearImports.detail('workspace-1', 'import-1'))).toBeUndefined()
})

test('start sends the team ids and stores the queued import', async () => {
  const calls = mockFetch(() => Response.json(linearImport({ status: 'queued' })))
  const { client, wrapper } = setupClient()
  const view = renderHook(() => useStartLinearImport('workspace-1'), { wrapper })
  await act(() => view.result.current.mutateAsync({ importId: 'import-1', teamIds: ['team-1', 'team-2'] }))
  expect(calls[0]).toEqual({ method: 'POST', path: `${base}/import-1/start`, search: '', body: { team_ids: ['team-1', 'team-2'] } })
  expect(client.getQueryData<LinearImport>(queryKeys.linearImports.detail('workspace-1', 'import-1'))?.status).toBe('queued')
})

test('cancel stores the cancelled import and refreshes projects', async () => {
  const calls = mockFetch(() => Response.json(linearImport({ status: 'cancelled' })))
  const { client, wrapper } = setupClient()
  client.setQueryData(queryKeys.projects('workspace-1'), [])
  const view = renderHook(() => useCancelLinearImport('workspace-1'), { wrapper })
  await act(() => view.result.current.mutateAsync({ importId: 'import-1' }))
  expect(calls[0]).toMatchObject({ method: 'POST', path: `${base}/import-1/cancel` })
  expect(client.getQueryData<LinearImport>(queryKeys.linearImports.detail('workspace-1', 'import-1'))?.status).toBe('cancelled')
  expect(client.getQueryState(queryKeys.projects('workspace-1'))?.isInvalidated).toBe(true)
})
