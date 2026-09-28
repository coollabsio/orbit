import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { queryKeys } from '@/api/queryKeys'
import type { ProjectRecord } from '@/api/generated/types.gen'
import { useUpdateProject } from './projects'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const project: ProjectRecord = {
  id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'ORB', color: '#e0457b',
  created_at: '', updated_at: '', version: 1, auto_close_parent: true, auto_close_sub_issues: true,
}

function withClient(client: QueryClient) {
  return ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
}

test('a successful project update writes the bumped version into the projects cache without waiting for a refetch', async () => {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'PATCH') {
      const body = (await request.json()) as { expected_version: number }
      return Response.json({ ...project, ...body, version: body.expected_version + 1 })
    }
    // The background refetch invalidateQueries triggers never resolves here: the cache write
    // on success must not depend on it.
    return new Promise<Response>(() => {})
  }) as unknown as typeof fetch

  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  client.setQueryData(queryKeys.projects('workspace-1'), [project])

  const { result } = renderHook(() => useUpdateProject('workspace-1', 'project-1'), { wrapper: withClient(client) })

  result.current.mutate({ name: 'Launch', key: 'ORB', color: '#e0457b', expected_version: 1, auto_close_parent: false })

  await waitFor(() => expect(result.current.isSuccess).toBeTrue())

  const cached = client.getQueryData<ProjectRecord[]>(queryKeys.projects('workspace-1'))
  expect(cached?.find((p) => p.id === 'project-1')?.version).toBe(2)
  expect(cached?.find((p) => p.id === 'project-1')?.auto_close_parent).toBeFalse()
})
