import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { Teamspace } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { queryKeys } from '@/api/queryKeys'
import {
  canDeleteTeamspaces,
  teamspaceDeleteError,
  useCreateTeamspace,
  useDeleteTeamspace,
  useRenameTeamspace,
  useTeamspaces,
} from './teamspaces'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const teamspace = (id: string, name: string, position: number, version = 1): Teamspace => ({
  id, workspace_id: 'workspace-1', name, icon: null, position, version, is_default: position === 0,
  created_at: '2026-09-25T10:00:00Z', updated_at: '2026-09-25T10:00:00Z',
})

const problem = (status: number, code: string) =>
  Response.json(
    { type: 'about:blank', title: 'Problem', status, code, detail: code, instance: '/teamspaces', request_id: 'request-1' },
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
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  return { client, wrapper }
}

const key = queryKeys.teamspaces('workspace-1')
const base = '/api/v1/workspaces/workspace-1/teamspaces'

test('the list reads the teamspaces endpoint; its key lives under the workspace prefix', async () => {
  const calls = mockFetch(() => Response.json({ items: [teamspace('t1', 'General', 0)] }))
  const { wrapper } = setupClient()
  const view = renderHook(() => useTeamspaces('workspace-1'), { wrapper })
  await waitFor(() => expect(view.result.current.data?.map((item) => item.name)).toEqual(['General']))
  expect(calls[0]).toMatchObject({ method: 'GET', path: base })
  expect(key.slice(0, 2)).toEqual(['workspace', 'workspace-1'])
})

test('create posts the name and appends the teamspace to the cache', async () => {
  const calls = mockFetch((call) =>
    call.method === 'POST' ? Response.json(teamspace('t2', 'Design', 1), { status: 201 }) : new Promise<Response>(() => {}),
  )
  const { client, wrapper } = setupClient()
  client.setQueryData(key, [teamspace('t1', 'General', 0)])
  const view = renderHook(() => useCreateTeamspace('workspace-1'), { wrapper })
  await act(async () => {
    await view.result.current.mutateAsync({ name: 'Design' })
  })
  expect(calls.find((call) => call.method === 'POST')).toMatchObject({ path: base, body: { name: 'Design' } })
  expect(client.getQueryData<Teamspace[]>(key)?.map((item) => item.id)).toEqual(['t1', 't2'])
})

test('rename patches the name with expected_version and replaces the cached teamspace', async () => {
  const calls = mockFetch((call) => (call.method === 'PATCH' ? Response.json(teamspace('t1', 'Team', 0, 2)) : new Promise<Response>(() => {})))
  const { client, wrapper } = setupClient()
  client.setQueryData(key, [teamspace('t1', 'General', 0)])
  const view = renderHook(() => useRenameTeamspace('workspace-1'), { wrapper })
  await act(async () => {
    await view.result.current.mutateAsync({ teamspaceId: 't1', version: 1, name: 'Team' })
  })
  expect(calls.find((call) => call.method === 'PATCH')).toMatchObject({ path: `${base}/t1`, body: { expected_version: 1, name: 'Team' } })
  expect(client.getQueryData<Teamspace[]>(key)?.[0]).toMatchObject({ name: 'Team', version: 2 })
})

test('delete sends expected_version and drops the teamspace; refusals keep it and read as sentences', async () => {
  let answer: Response = new Response(null, { status: 204 })
  const calls = mockFetch((call) => (call.method === 'DELETE' ? answer : new Promise<Response>(() => {})))
  const { client, wrapper } = setupClient()
  client.setQueryData(key, [teamspace('t1', 'General', 0), teamspace('t2', 'Design', 1)])
  const view = renderHook(() => useDeleteTeamspace('workspace-1'), { wrapper })

  answer = problem(409, 'teamspace_not_empty')
  let error: unknown
  await act(async () => {
    error = await view.result.current.mutateAsync({ teamspaceId: 't2', version: 1 }).catch((caught: unknown) => caught)
  })
  expect(error).toBeInstanceOf(ApiProblem)
  expect(teamspaceDeleteError(error)).toContain('Move or delete its pages')
  expect(client.getQueryData<Teamspace[]>(key)).toHaveLength(2)

  answer = new Response(null, { status: 204 })
  await act(async () => {
    await view.result.current.mutateAsync({ teamspaceId: 't2', version: 1 })
  })
  expect(calls.filter((call) => call.method === 'DELETE').at(-1)).toMatchObject({ path: `${base}/t2`, search: '?expected_version=1' })
  expect(client.getQueryData<Teamspace[]>(key)?.map((item) => item.id)).toEqual(['t1'])
})

test('error sentences and role gate', () => {
  expect(teamspaceDeleteError(new ApiProblem({ type: 'about:blank', title: 'x', status: 422, code: 'last_teamspace', detail: 'x', instance: '/', request_id: 'r' }))).toBe(
    'A workspace keeps at least one teamspace.',
  )
  expect(teamspaceDeleteError(new ApiProblem({ type: 'about:blank', title: 'x', status: 403, code: 'workspace_action_forbidden', detail: 'x', instance: '/', request_id: 'r' }))).toContain(
    'owners and admins',
  )
  expect(teamspaceDeleteError(new Error('offline'))).toBe('Could not delete the teamspace.')
  expect(canDeleteTeamspaces('owner')).toBeTrue()
  expect(canDeleteTeamspaces('admin')).toBeTrue()
  expect(canDeleteTeamspaces('member')).toBeFalse()
})
