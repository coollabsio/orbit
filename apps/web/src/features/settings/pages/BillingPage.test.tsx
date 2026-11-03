import { afterEach, expect, mock, spyOn, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import type { BillingView, PlanView } from '@orbit/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { testWorkspace } from '@/test/workspace'
import { BillingPage } from './BillingPage'

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
})

function price(key: string, name: string, amount: number, interval: 'month' | 'year'): PlanView {
  return { key, product_id: `prod_${name.toLowerCase()}`, name, amount, currency: 'usd', interval }
}

const plans = [
  price('team-monthly', 'Team', 1000, 'month'),
  price('team-annual', 'Team', 10000, 'year'),
  price('business-monthly', 'Business', 2900, 'month'),
  price('business-annual', 'Business', 29000, 'year'),
]

const readOnly: BillingView = {
  plan: null,
  status: null,
  access: 'read_only',
  exempt: false,
  cancel_at_period_end: false,
  current_period_end: null,
  plans,
  has_customer: false,
  can_manage: true,
  trial_days: 0,
  trial: null,
}

function serve(billing: BillingView, requests: Request[] = []) {
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    requests.push(request)
    const path = new URL(request.url).pathname
    if (path === '/api/v1/auth/options') {
      return Response.json({ registration_open: false, email_enabled: false, passkeys_enabled: false, billing_enabled: true })
    }
    if (path.endsWith('/billing/checkout')) return Response.json({ url: 'https://checkout.stripe.test/cs' })
    if (path.endsWith('/billing/portal')) return Response.json({ url: 'https://billing.stripe.test/p' })
    if (path.endsWith('/billing')) return Response.json(billing)
    return Response.json({}, { status: 404 })
  }) as unknown as typeof fetch
}

function renderPage(role: 'owner' | 'member') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const workspace = testWorkspace(role)
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/settings/billing']}>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
          <BillingPage />
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

test('the owner of a read-only workspace picks a plan and goes to Stripe Checkout', async () => {
  const requests: Request[] = []
  serve(readOnly, requests)
  const assign = spyOn(window.location, 'assign').mockImplementation(() => {})
  const view = renderPage('owner')

  await waitFor(() => expect(view.getByText('Read-only')).toBeTruthy())
  expect(view.getByText(/^Each workspace has its own plan\. Without an active plan, the workspace is read-only/)).toBeTruthy()
  expect(view.getByText('Not subscribed')).toBeTruthy()
  expect(view.queryAllByRole('button', { name: 'Manage billing' })).toHaveLength(0)
  expect(view.getByText('$10')).toBeTruthy()
  expect(view.getByText('$29')).toBeTruthy()
  expect(view.getAllByText('per workspace / month')).toHaveLength(2)

  await userEvent.click(view.getByRole('button', { name: 'Annual · save 16%' }))
  expect(view.getByText('$100')).toBeTruthy()
  expect(view.getByText('$290')).toBeTruthy()
  expect(view.getByText('$8.33 / month, billed yearly')).toBeTruthy()
  expect(view.queryAllByText('$10')).toHaveLength(0)
  await userEvent.click(view.getByRole('button', { name: 'Choose Team' }))

  await waitFor(() => expect(assign).toHaveBeenCalledWith('https://checkout.stripe.test/cs'))
  const checkout = requests.find((request) => request.method === 'POST')!
  expect(new URL(checkout.url).pathname).toBe(`/api/v1/workspaces/${testWorkspace('owner').id}/billing/checkout`)
  expect(await checkout.json()).toEqual({ plan: 'team-annual' })
  assign.mockRestore()
})

test('the owner of a trial workspace sees when it ends and its usage, and can choose a plan', async () => {
  serve({
    ...readOnly,
    access: 'trial',
    trial_days: 14,
    trial: {
      ends_at: '2030-03-17T12:00:00.000Z',
      tasks: { used: 12, limit: 50 },
      pages: { used: 3, limit: 20 },
      members: { used: 1, limit: 3 },
    },
  })
  const view = renderPage('owner')

  await waitFor(() => expect(view.getByText('Trial')).toBeTruthy())
  expect(view.getByText(/A new workspace starts with a 14-day trial\. Without an active plan after it/)).toBeTruthy()
  expect(view.getByText('Sun, Mar 17, 2030')).toBeTruthy()
  expect(view.getAllByRole('listitem').map((item) => item.textContent)).toEqual(['12 of 50 tasks', '3 of 20 pages', '1 of 3 members'])
  expect(view.getAllByRole('button').map((button) => button.textContent)).toEqual(['Monthly', 'Annual · save 16%', 'Choose Team', 'Choose Business'])
})

test('a subscribed owner sees the renewal date and opens the billing portal', async () => {
  serve({ ...readOnly, plan: 'team-monthly', status: 'active', access: 'full', has_customer: true, current_period_end: '2030-03-17T17:46:40.000Z' })
  const assign = spyOn(window.location, 'assign').mockImplementation(() => {})
  const view = renderPage('owner')

  await waitFor(() => expect(view.getByText('Full access')).toBeTruthy())
  expect(view.getByText('Team · Monthly')).toBeTruthy()
  expect(view.getByText('Active')).toBeTruthy()
  expect(view.getByText(/^Renews on /)).toBeTruthy()
  expect(view.queryAllByRole('button', { name: 'Choose Team' })).toHaveLength(0)
  await userEvent.click(view.getByRole('button', { name: 'Manage billing' }))
  await waitFor(() => expect(assign).toHaveBeenCalledWith('https://billing.stripe.test/p'))
  assign.mockRestore()
})

test('the exempt workspace needs no subscription and offers no plans', async () => {
  serve({ ...readOnly, access: 'full', exempt: true })
  const view = renderPage('owner')

  await waitFor(() => expect(view.getByText('Full access')).toBeTruthy())
  expect(view.getByText('Included')).toBeTruthy()
  expect(view.getByText('No subscription needed')).toBeTruthy()
  expect(view.queryByText('Choose a plan')).toBeNull()
  expect(view.queryAllByRole('button')).toHaveLength(0)
})

test('members see the prices but cannot choose a plan', async () => {
  serve({ ...readOnly, can_manage: false, has_customer: true })
  const view = renderPage('member')

  await waitFor(() => expect(view.getByText('Only the workspace owner can choose a plan.')).toBeTruthy())
  expect(view.getByText('$10')).toBeTruthy()
  expect(view.getAllByRole('button').map((button) => button.textContent)).toEqual(['Monthly', 'Annual · save 16%'])
})

test('without prices from Stripe each plan offers its name and a button', async () => {
  const fallback = (key: string, name: string): PlanView => ({ key, product_id: null, name, amount: null, currency: null, interval: null })
  serve({ ...readOnly, plans: [fallback('team', 'Team'), fallback('business', 'Business')] })
  const view = renderPage('owner')

  await waitFor(() => expect(view.getByText('Read-only')).toBeTruthy())
  expect(view.getAllByRole('button').map((button) => button.textContent)).toEqual(['Choose Team', 'Choose Business'])
  expect(view.queryAllByText('Unlimited tasks, pages and members')).toHaveLength(0)
})
