import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { queryKeys } from '@/api/queryKeys'
import { registerConfirmationHandler } from '@/components/common/confirmAction'
import { AdminUsersPage } from './AdminUsersPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const USERS = [
  { id: 'root-1', email: 'root@orbit.test', display_name: 'Root Person', root: true, admin: true, suspended: false, created_at: '2026-10-01T10:00:00.000Z', last_active_at: '2026-10-08T10:00:00.000Z', workspace_count: 2, two_factor: false },
  { id: 'user-2', email: 'ada@orbit.test', display_name: 'Ada', root: false, admin: false, suspended: false, created_at: '2026-10-02T10:00:00.000Z', last_active_at: null, workspace_count: 1, two_factor: true },
  { id: 'user-4', email: 'cy@orbit.test', display_name: 'Cy', root: false, admin: true, suspended: false, created_at: '2026-10-02T11:00:00.000Z', last_active_at: null, workspace_count: 1, two_factor: false },
  { id: 'user-3', email: 'bob@orbit.test', display_name: 'Bob', root: false, admin: false, suspended: true, created_at: '2026-10-03T10:00:00.000Z', last_active_at: null, workspace_count: 0, two_factor: false },
]

/** The viewer is the root user unless `viewer` says otherwise. */
function renderPage(requests: string[] = [], viewer: { id: string; root: boolean } = { id: 'root-1', root: true }) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    requests.push(`${request.method} ${url.pathname}${url.search}`)
    if (request.method === 'POST' && url.pathname.endsWith('/two-factor/reset')) return new Response(null, { status: 204 })
    if (request.method === 'POST') {
      return Response.json({ url: 'https://orbit.test/recovery?token=secret', expires_at: '2026-10-08T10:30:00.000Z' }, { status: 201 })
    }
    const q = url.searchParams.get('q')?.toLowerCase() ?? ''
    return Response.json({ items: USERS.filter((user) => `${user.display_name} ${user.email}`.toLowerCase().includes(q)), next_cursor: null })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.currentUser, { id: viewer.id, email: 'viewer@orbit.test', display_name: 'Viewer', installation_admin: true, root: viewer.root, status: { presence: 'online' } })
  return render(<QueryClientProvider client={client}><AdminUsersPage /></QueryClientProvider>)
}

test('the root account has no actions and a suspended account can only be reinstated', async () => {
  const view = renderPage()
  await view.findByText('Root Person')
  expect(view.queryByRole('button', { name: 'Manage Root Person' })).toBeNull()
  expect(view.getByText('Root')).toBeTruthy()
  expect(view.getByText('Suspended')).toBeTruthy()

  fireEvent.click(view.getByRole('button', { name: 'Manage Bob' }))
  expect(await view.findByRole('menuitem', { name: 'Reinstate account' })).toBeTruthy()
  expect(view.queryAllByRole('menuitem', { name: 'Create password recovery link' })).toHaveLength(0)
})

test('a recovery link opens in a dialog with the URL to copy', async () => {
  const requests: string[] = []
  const view = renderPage(requests)
  await view.findByText('Ada')
  fireEvent.click(view.getByRole('button', { name: 'Manage Ada' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Create password recovery link' }))
  const field = await view.findByLabelText('Recovery link') as HTMLInputElement
  expect(field.value).toBe('https://orbit.test/recovery?token=secret')
  expect(view.getByText(/Send this link to ada@orbit.test/)).toBeTruthy()
  expect(requests).toContain('POST /api/v1/admin/users/user-2/recovery-link')
})

test('the search asks the server for users by name or email', async () => {
  const requests: string[] = []
  const view = renderPage(requests)
  await view.findByText('Ada')
  const search = view.getByRole('searchbox', { name: 'Search users' })
  fireEvent.change(search, { target: { value: ' BOB@orbit ' } })
  await waitFor(() => expect(view.queryByText('Ada') === null).toBe(true))
  expect(view.getByText('Bob')).toBeTruthy()
  expect(requests).toContain('GET /api/v1/admin/users?q=BOB%40orbit')
  fireEvent.change(search, { target: { value: 'nobody' } })
  expect(await view.findByText('No user matches "nobody".')).toBeTruthy()
})

test('only the root user manages admins; an admin cannot open the menu of another admin or their own row', async () => {
  const requests: string[] = []
  const root = renderPage(requests)
  await root.findByText('Cy')
  expect(root.getByText('Admin')).toBeTruthy()
  fireEvent.click(root.getByRole('button', { name: 'Manage Ada' }))
  expect(await root.findByRole('menuitem', { name: 'Make admin' })).toBeTruthy()
  root.unmount()

  const admin = renderPage([], { id: 'user-4', root: false })
  await admin.findByText('Ada')
  expect(admin.queryByRole('button', { name: 'Manage Cy' })).toBeNull()
  expect(admin.queryByRole('button', { name: 'Manage Root Person' })).toBeNull()
  fireEvent.click(admin.getByRole('button', { name: 'Manage Ada' }))
  expect(await admin.findByRole('menuitem', { name: 'Create password recovery link' })).toBeTruthy()
  expect(admin.queryAllByRole('menuitem', { name: 'Make admin' })).toHaveLength(0)
})

test('an account with two-factor sign-in shows it and the admin can reset it after confirming', async () => {
  const requests: string[] = []
  const confirmations: string[] = []
  const unregister = registerConfirmationHandler(async ({ title }) => { confirmations.push(title); return true })
  try {
    const view = renderPage(requests)
    await view.findByText('Ada')
    expect(view.getAllByText('2FA')).toHaveLength(1)
    fireEvent.click(view.getByRole('button', { name: 'Manage Cy' }))
    await view.findByRole('menuitem', { name: 'Create password recovery link' })
    expect(view.queryAllByRole('menuitem', { name: 'Reset two-factor' })).toHaveLength(0)
    view.unmount()

    const again = renderPage(requests)
    await again.findByText('Ada')
    fireEvent.click(again.getByRole('button', { name: 'Manage Ada' }))
    await userEvent.click(await again.findByRole('menuitem', { name: 'Reset two-factor' }))
    await waitFor(() => expect(requests).toContain('POST /api/v1/admin/users/user-2/two-factor/reset'))
    expect(confirmations).toEqual(['Reset two-factor sign-in for Ada?'])
  } finally {
    unregister()
  }
})
