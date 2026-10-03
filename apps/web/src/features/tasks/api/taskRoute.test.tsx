import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { ProjectRecord, TaskRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { cachedTaskSlug, useTaskRouteId } from './tasks'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const id = '0190f5b4-7c1e-7a3b-8c4d-5e6f7a8b9c0d'
const record = { id, workspace_id: 'workspace-1', project_id: 'project-1', number: 12, title: 'Ship it' } as TaskRecord
const project = { id: 'project-1', key: 'ENG' } as ProjectRecord

function setup() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  return { client, wrapper }
}

test('an id route segment is the task id, without a request', () => {
  let calls = 0
  globalThis.fetch = (async () => {
    calls += 1
    return Response.json(record)
  }) as unknown as typeof fetch
  const { wrapper } = setup()
  const view = renderHook(() => useTaskRouteId('workspace-1', id.toUpperCase()), { wrapper })
  expect(view.result.current).toEqual({ taskId: id, error: null })
  expect(calls).toBe(0)
})

test('an identifier resolves through one GET that also fills the detail cache', async () => {
  const paths: string[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    paths.push(new URL(input instanceof Request ? input.url : String(input), 'http://orbit.test').pathname)
    return Response.json(record)
  }) as unknown as typeof fetch
  const { client, wrapper } = setup()
  const view = renderHook(() => useTaskRouteId('workspace-1', 'eng-12'), { wrapper })
  expect(view.result.current.taskId).toBeUndefined()
  await waitFor(() => expect(view.result.current.taskId).toBe(id))
  expect(paths).toEqual(['/api/v1/workspaces/workspace-1/tasks/eng-12'])
  expect(client.getQueryData<TaskRecord>(queryKeys.tasks.detail('workspace-1', id))?.title).toBe('Ship it')
})

test('an unknown identifier reports the error', async () => {
  globalThis.fetch = (async () => Response.json(
    { type: 'about:blank', title: 'Task resource not found', status: 404, code: 'task_resource_not_found', detail: 'The requested task resource was not found.', instance: '/x', request_id: 'r' },
    { status: 404, headers: { 'content-type': 'application/problem+json' } },
  )) as unknown as typeof fetch
  const { wrapper } = setup()
  const view = renderHook(() => useTaskRouteId('workspace-1', 'ENG-99'), { wrapper })
  await waitFor(() => expect(view.result.current.error).not.toBeNull())
  expect(view.result.current.taskId).toBeUndefined()
})

test('a cached task opens by identifier, primed so the new URL needs no request', () => {
  let calls = 0
  globalThis.fetch = (async () => {
    calls += 1
    return Response.json(record)
  }) as unknown as typeof fetch
  const { client, wrapper } = setup()
  // nothing cached yet: the id
  expect(cachedTaskSlug(client, 'workspace-1', id)).toBe(id)
  client.setQueryData(queryKeys.tasks.detail('workspace-1', id), record)
  client.setQueryData(queryKeys.projects('workspace-1'), [project])
  expect(cachedTaskSlug(client, 'workspace-1', id)).toBe('ENG-12')
  const view = renderHook(() => useTaskRouteId('workspace-1', 'ENG-12'), { wrapper })
  expect(view.result.current.taskId).toBe(id)
  expect(calls).toBe(0)
})
