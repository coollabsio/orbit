import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import type { BillingView } from '@orbit/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { testWorkspace } from '@/test/workspace'
import { ReadOnlyBanner } from './ReadOnlyBanner'

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
})

const DAY = 24 * 60 * 60 * 1000

function renderBanner(billing: BillingView) {
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = new URL(new Request(input, init).url).pathname
    if (path === '/api/v1/auth/options') {
      return Response.json({ registration_open: false, email_enabled: false, passkeys_enabled: false, billing_enabled: true })
    }
    if (path.endsWith('/billing')) return Response.json(billing)
    return Response.json({}, { status: 404 })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const workspace = testWorkspace('member')
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
          <ReadOnlyBanner />
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

test('a trial workspace shows the days left with a link to billing', async () => {
  const usage = { used: 0, limit: 1 }
  const view = renderBanner({
    plan: null,
    status: null,
    access: 'trial',
    exempt: false,
    cancel_at_period_end: false,
    current_period_end: null,
    plans: [{ key: 'team', name: 'Team' }],
    has_customer: false,
    can_manage: false,
    trial_days: 7,
    trial: { ends_at: new Date(Date.now() + 2.5 * DAY).toISOString(), tasks: usage, pages: usage, members: usage },
  })

  await waitFor(() => expect(view.getByRole('status').textContent).toBe('Trial: 3 days leftView billing'))
  expect(view.getByRole('link', { name: 'View billing' }).getAttribute('href')).toBe('/settings/billing')
})
