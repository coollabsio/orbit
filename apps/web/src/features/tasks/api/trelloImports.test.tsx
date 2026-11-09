import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { TrelloImport, TrelloImportTree } from '@orbit/api/generated/types.gen'
import { createApiClient } from '@/api/client'
import { ApiProblem } from '@orbit/api/problem'
import { queryKeys } from '@/api/queryKeys'
import {
  trelloCredentialField,
  trelloImportErrorMessage,
  createTrelloImportRequest,
  useTrelloImport,
  useTrelloImportTree,
  useCancelTrelloImport,
  useStartTrelloImport,
} from './trelloImports'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const trelloImport = (patch: Partial<TrelloImport> = {}): TrelloImport => ({
  id: 'import-1', workspace_id: 'workspace-1', status: 'scanning', trello_user_name: 'Ada', error: null,
  progress: { total: 0, done: 0, failed: 0 }, tree: null, report: null, project_ids: [],
  created_at: '2026-11-04T10:00:00Z', updated_at: '2026-11-04T10:00:00Z', ...patch,
})

const problem = (status: number, code: string, detail = code) =>
  new ApiProblem({ type: 'about:blank', title: 'Problem', status, code, detail, instance: '/imports', request_id: 'request-1' })

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

const base = '/api/v1/workspaces/workspace-1/imports/trello'

test('problems map to friendly messages', () => {
  const keyDetail = 'Trello rejected this API key (invalid key). Copy the key again from the Trello Auth tab of your Power-Up.'
  expect(trelloImportErrorMessage(problem(422, 'trello_key_invalid', keyDetail))).toBe(keyDetail)
  expect(trelloImportErrorMessage(problem(422, 'trello_token_invalid', ''))).toBe(
    'Trello rejected this token. Generate a new token for this API key and paste it.',
  )
  expect(trelloImportErrorMessage(problem(502, 'trello_unavailable'))).toBe('Trello could not be reached. Try again in a moment.')
  expect(trelloImportErrorMessage(problem(409, 'import_in_progress'))).toBe('You already have an import running. Wait for it to finish or cancel it.')
  expect(trelloImportErrorMessage(problem(422, 'validation_failed'))).toBe('The request was not valid. Check your input and try again.')
  expect(trelloImportErrorMessage(new Error('boom'))).toBe('Something went wrong. Try again.')
})

test('rejected credentials name the field they concern', () => {
  expect(trelloCredentialField(problem(422, 'trello_key_invalid'))).toBe('key')
  expect(trelloCredentialField(problem(422, 'trello_token_invalid'))).toBe('token')
  expect(trelloCredentialField(problem(502, 'trello_unavailable'))).toBeNull()
  expect(trelloCredentialField(new Error('boom'))).toBeNull()
})

test('the key and token are posted once to the collection', async () => {
  const calls = mockFetch(() => Response.json(trelloImport(), { status: 201 }))
  const created = await createTrelloImportRequest(createApiClient(), 'workspace-1', { key: 'key1', token: 'token1' })
  expect(created.id).toBe('import-1')
  expect(calls).toEqual([{ method: 'POST', path: base, search: '', body: { key: 'key1', token: 'token1' } }])
})

test('an import is polled while it works and refreshes projects and tasks when it ends', async () => {
  let status: TrelloImport['status'] = 'importing'
  const calls = mockFetch(() => Response.json(trelloImport({ status })))
  const polls = () => calls.filter((call) => call.path === `${base}/import-1`).length
  const { client, wrapper } = setupClient()
  client.setQueryData(queryKeys.projects('workspace-1'), [])
  client.setQueryData(queryKeys.tasks.list('workspace-1'), [])
  const view = renderHook(() => useTrelloImport('workspace-1', 'import-1', 20), { wrapper })
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
  const tree: TrelloImportTree = {
    workspaces: [{ id: 'ws-1', name: 'Acme', boards: [{ id: 'board-1', name: 'Roadmap', card_count: 12, archived: false }] }],
    truncated: false,
  }
  const calls = mockFetch((call) => Response.json(trelloImport({ status: 'ready', tree: call.search === '?include=tree' ? tree : null })))
  const { client, wrapper } = setupClient()
  const view = renderHook(() => useTrelloImportTree('workspace-1', 'import-1'), { wrapper })
  await waitFor(() => expect(view.result.current.data).toEqual(tree))
  await act(() => new Promise((resolve) => setTimeout(resolve, 80)))
  expect(calls).toEqual([{ method: 'GET', path: `${base}/import-1`, search: '?include=tree', body: undefined }])
  expect(client.getQueryData(queryKeys.trelloImports.detail('workspace-1', 'import-1'))).toBeUndefined()
})

test('start sends the board ids and stores the queued import', async () => {
  const calls = mockFetch(() => Response.json(trelloImport({ status: 'queued' })))
  const { client, wrapper } = setupClient()
  const view = renderHook(() => useStartTrelloImport('workspace-1'), { wrapper })
  await act(() => view.result.current.mutateAsync({ importId: 'import-1', boardIds: ['board-1', 'board-2'] }))
  expect(calls[0]).toEqual({ method: 'POST', path: `${base}/import-1/start`, search: '', body: { board_ids: ['board-1', 'board-2'] } })
  expect(client.getQueryData<TrelloImport>(queryKeys.trelloImports.detail('workspace-1', 'import-1'))?.status).toBe('queued')
})

test('cancel stores the cancelled import and refreshes projects', async () => {
  const calls = mockFetch(() => Response.json(trelloImport({ status: 'cancelled' })))
  const { client, wrapper } = setupClient()
  client.setQueryData(queryKeys.projects('workspace-1'), [])
  const view = renderHook(() => useCancelTrelloImport('workspace-1'), { wrapper })
  await act(() => view.result.current.mutateAsync({ importId: 'import-1' }))
  expect(calls[0]).toMatchObject({ method: 'POST', path: `${base}/import-1/cancel` })
  expect(client.getQueryData<TrelloImport>(queryKeys.trelloImports.detail('workspace-1', 'import-1'))?.status).toBe('cancelled')
  expect(client.getQueryState(queryKeys.projects('workspace-1'))?.isInvalidated).toBe(true)
})
