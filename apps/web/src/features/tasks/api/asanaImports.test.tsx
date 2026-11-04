import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { AsanaImport, AsanaImportTree } from '@orbit/api/generated/types.gen'
import { createApiClient } from '@/api/client'
import { ApiProblem } from '@orbit/api/problem'
import { queryKeys } from '@/api/queryKeys'
import {
  asanaImportErrorMessage,
  createAsanaImportRequest,
  isActiveAsanaImport,
  isFinalAsanaImport,
  useAsanaImport,
  useAsanaImportTree,
  useCancelAsanaImport,
  useStartAsanaImport,
} from './asanaImports'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const asanaImport = (patch: Partial<AsanaImport> = {}): AsanaImport => ({
  id: 'import-1', workspace_id: 'workspace-1', status: 'scanning', asana_user_name: 'Ada', error: null,
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

const base = '/api/v1/workspaces/workspace-1/imports/asana'

test('status helpers: active imports poll, final ones never change', () => {
  expect(['scanning', 'queued', 'importing'].map((status) => isActiveAsanaImport(status as AsanaImport['status']))).toEqual([true, true, true])
  expect(isActiveAsanaImport('ready')).toBe(false)
  expect(['completed', 'failed', 'cancelled', 'expired'].map((status) => isFinalAsanaImport(status as AsanaImport['status']))).toEqual([
    true, true, true, true,
  ])
  expect(isFinalAsanaImport('ready')).toBe(false)
})

test('problems map to friendly messages', () => {
  expect(asanaImportErrorMessage(problem(422, 'asana_token_invalid'))).toBe(
    'Asana rejected this token. Check that you copied the whole token and that it was not revoked.',
  )
  expect(asanaImportErrorMessage(problem(502, 'asana_unavailable'))).toBe('Asana could not be reached. Try again in a moment.')
  expect(asanaImportErrorMessage(problem(409, 'import_in_progress'))).toBe('You already have an import running. Wait for it to finish or cancel it.')
  expect(asanaImportErrorMessage(problem(422, 'validation_failed'))).toBe('The request was not valid. Check your input and try again.')
  expect(asanaImportErrorMessage(new Error('boom'))).toBe('Something went wrong. Try again.')
})

test('the token is posted once to the collection', async () => {
  const calls = mockFetch(() => Response.json(asanaImport(), { status: 201 }))
  const created = await createAsanaImportRequest(createApiClient(), 'workspace-1', '2/secret')
  expect(created.id).toBe('import-1')
  expect(calls).toEqual([{ method: 'POST', path: base, search: '', body: { token: '2/secret' } }])
})

test('an import is polled while it works and refreshes projects and tasks when it ends', async () => {
  let status: AsanaImport['status'] = 'importing'
  const calls = mockFetch(() => Response.json(asanaImport({ status })))
  const polls = () => calls.filter((call) => call.path === `${base}/import-1`).length
  const { client, wrapper } = setupClient()
  client.setQueryData(queryKeys.projects('workspace-1'), [])
  client.setQueryData(queryKeys.tasks.list('workspace-1'), [])
  const view = renderHook(() => useAsanaImport('workspace-1', 'import-1', 20), { wrapper })
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
  const tree: AsanaImportTree = {
    workspaces: [{ gid: '100', name: 'Acme', projects: [{ gid: '200', name: 'Launch', archived: false, team_gid: '9', team_name: 'Eng' }] }],
    truncated: false,
  }
  const calls = mockFetch((call) => Response.json(asanaImport({ status: 'ready', tree: call.search === '?include=tree' ? tree : null })))
  const { client, wrapper } = setupClient()
  const view = renderHook(() => useAsanaImportTree('workspace-1', 'import-1'), { wrapper })
  await waitFor(() => expect(view.result.current.data).toEqual(tree))
  await act(() => new Promise((resolve) => setTimeout(resolve, 80)))
  expect(calls).toEqual([{ method: 'GET', path: `${base}/import-1`, search: '?include=tree', body: undefined }])
  expect(client.getQueryData(queryKeys.asanaImports.detail('workspace-1', 'import-1'))).toBeUndefined()
})

test('start sends the project gids and stores the queued import', async () => {
  const calls = mockFetch(() => Response.json(asanaImport({ status: 'queued' })))
  const { client, wrapper } = setupClient()
  const view = renderHook(() => useStartAsanaImport('workspace-1'), { wrapper })
  await act(() => view.result.current.mutateAsync({ importId: 'import-1', projectGids: ['200', '201'] }))
  expect(calls[0]).toEqual({ method: 'POST', path: `${base}/import-1/start`, search: '', body: { project_gids: ['200', '201'] } })
  expect(client.getQueryData<AsanaImport>(queryKeys.asanaImports.detail('workspace-1', 'import-1'))?.status).toBe('queued')
})

test('cancel stores the cancelled import and refreshes projects', async () => {
  const calls = mockFetch(() => Response.json(asanaImport({ status: 'cancelled' })))
  const { client, wrapper } = setupClient()
  client.setQueryData(queryKeys.projects('workspace-1'), [])
  const view = renderHook(() => useCancelAsanaImport('workspace-1'), { wrapper })
  await act(() => view.result.current.mutateAsync({ importId: 'import-1' }))
  expect(calls[0]).toMatchObject({ method: 'POST', path: `${base}/import-1/cancel` })
  expect(client.getQueryData<AsanaImport>(queryKeys.asanaImports.detail('workspace-1', 'import-1'))?.status).toBe('cancelled')
  expect(client.getQueryState(queryKeys.projects('workspace-1'))?.isInvalidated).toBe(true)
})
