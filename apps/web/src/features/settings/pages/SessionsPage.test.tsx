import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { queryKeys } from '@/api/queryKeys'
import { sessionView } from '@/features/settings/api/sessions'
import { SessionsPage } from './SessionsPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const record = (id: string, current: boolean) => sessionView({
  id, current, created_at: '2026-09-05T10:00:00Z', last_activity_at: '2026-09-05T10:00:00Z',
  idle_expires_at: '2026-09-06T10:00:00Z', absolute_expires_at: '2026-10-05T10:00:00Z',
  user: { id: 'user-1', email: 'user@orbit.test', display_name: 'User' },
})

test('mounted sessions never offer to revoke the server-identified current session', () => {
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } })
  client.setQueryData(queryKeys.sessions, [record('current', true), record('other', false)])
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  const view = render(<SessionsPage />, { wrapper })

  expect(view.getAllByRole('button', { name: 'Revoke' })).toHaveLength(1)
  expect(view.getByText('Current')).toBeTruthy()
})

test('sign out all sequences requests and reports and retries every partial failure', async () => {
  const records = [record('current', true), record('other-1', false), record('other-2', false)]
  const remaining = new Set(['current', 'other-1', 'other-2'])
  const attempts = new Map<string, number>()
  const revoked: string[] = []
  let active = 0
  let maxActive = 0
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'GET') return Response.json(records.filter((session) => remaining.has(session.id)))
    const id = request.url.split('/').at(-1)!
    revoked.push(id)
    active += 1
    maxActive = Math.max(maxActive, active)
    await new Promise((resolve) => setTimeout(resolve, 5))
    active -= 1
    const attempt = (attempts.get(id) ?? 0) + 1
    attempts.set(id, attempt)
    if (id === 'other-1' && attempt === 1) return Response.json({
      type: 'about:blank', title: 'Failed', status: 500, detail: 'offline', code: 'failed',
      instance: request.url, request_id: 'request-1',
    }, { status: 500, headers: { 'content-type': 'application/problem+json' } })
    remaining.delete(id)
    return new Response(null, { status: 204 })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false }, mutations: { retry: false } } })
  client.setQueryData(queryKeys.sessions, records)
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  const view = render(<SessionsPage />, { wrapper })

  fireEvent.click(view.getByRole('button', { name: 'Sign out all other sessions' }))
  expect((await view.findByRole('status')).textContent).toContain('Revoking 2 sessions')
  expect((await view.findByRole('alert')).textContent).toContain('1 of 2 sessions could not be revoked')
  expect(maxActive).toBe(1)
  expect(revoked).toEqual(['other-1', 'other-2'])

  fireEvent.click(view.getByRole('button', { name: 'Retry failed sessions' }))
  await waitFor(() => expect(view.queryByRole('alert')).toBeNull())
  expect(revoked).toEqual(['other-1', 'other-2', 'other-1'])
})
