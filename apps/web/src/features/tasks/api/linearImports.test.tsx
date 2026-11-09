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
  linearImportProgress,
  linearRateLimitNotice,
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
  progress: { total: 0, done: 0, failed: 0, expected: 0, read: 0, created: 0, stage: null }, waiting_until: null, tree: null, report: null, project_ids: [],
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

test('progress names each stage with its own count', () => {
  const at = (stage: LinearImport['progress']['stage'], patch: Partial<LinearImport['progress']> = {}) =>
    linearImportProgress(linearImport({ status: 'importing', progress: { total: 8_000, done: 0, failed: 0, expected: 8_005, read: 0, created: 0, stage, ...patch } }))

  expect(linearImportProgress(linearImport({ status: 'queued' }))).toEqual({ label: 'Waiting for the import to start…', step: null, done: 0, total: 0, detail: '' })
  expect(at('reading', { total: 605, read: 1_230 })).toEqual({ label: 'Reading issues from Linear…', step: 1, done: 1_230, total: 8_005, detail: '1,230 of about 8,005 issues read' })
  expect(at('reading', { read: 8_100 }).total).toBe(8_100)
  expect(at('creating', { created: 2_500 })).toEqual({ label: 'Creating tasks…', step: 2, done: 2_500, total: 8_000, detail: '2,500 of 8,000 tasks created' })
  expect(at('details', { done: 40, failed: 2 })).toEqual({ label: 'Adding files and comments…', step: 3, done: 42, total: 8_000, detail: '40 of 8,000 issues imported' })
  expect(at('linking', { done: 7_990, failed: 10 }).step).toBe(4)
  expect(at(null, { done: 3 }).label).toBe('Importing issues…')
})

test('the rate-limit notice shows only while the wait lasts', () => {
  const now = Date.parse('2026-11-09T21:00:00Z')
  expect(linearRateLimitNotice(null, now)).toBeNull()
  expect(linearRateLimitNotice('2026-11-09T20:59:00Z', now)).toBeNull()
  expect(linearRateLimitNotice('2026-11-09T21:11:30Z', now)).toMatch(/^Linear's rate limit is reached\. The import goes on at .+ \(in about 12 min\)\.$/)
  expect(linearRateLimitNotice('2026-11-09T21:00:10Z', now)).toMatch(/\(in about 1 min\)/)
})
