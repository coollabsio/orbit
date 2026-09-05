import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { useTransferOwnership } from './api'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

test('ownership transfer hook sends both protected versions to the dedicated operation', async () => {
  let body: unknown
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    body = await request.json()
    return new Response(null, { status: 204 })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  const view = renderHook(() => useTransferOwnership('workspace-1'), { wrapper })

  await act(async () => {
    await view.result.current.mutateAsync({ membershipId: 'membership-2', membershipVersion: 4, workspaceVersion: 7 })
  })
  await waitFor(() => expect(view.result.current.isSuccess).toBeTrue())

  expect(body).toEqual({ membership_id: 'membership-2', membership_version: 4, expected_version: 7 })
})
