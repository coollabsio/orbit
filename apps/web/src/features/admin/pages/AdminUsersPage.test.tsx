import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Toaster } from 'sonner'
import type { AdminUser, AuthOptions } from '@orbit/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { registerConfirmationHandler } from '@/components/common/confirmAction'
import { AdminUsersPage } from './AdminUsersPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const USERS: AdminUser[] = [
  { id: 'root-1', email: 'root@orbit.test', display_name: 'Root Person', root: true, admin: true, suspended: false, created_at: '2026-10-01T10:00:00.000Z', last_active_at: '2026-10-08T10:00:00.000Z', workspace_count: 2, deleted: false, two_factor: false },
  { id: 'user-2', email: 'ada@orbit.test', display_name: 'Ada', root: false, admin: false, suspended: false, created_at: '2026-10-02T10:00:00.000Z', last_active_at: null, workspace_count: 1, two_factor: true, deleted: false },
  { id: 'user-4', email: 'cy@orbit.test', display_name: 'Cy', root: false, admin: true, suspended: false, created_at: '2026-10-02T11:00:00.000Z', last_active_at: null, workspace_count: 1, two_factor: false, deleted: false },
  { id: 'user-3', email: 'bob@orbit.test', display_name: 'Bob', root: false, admin: false, suspended: true, created_at: '2026-10-03T10:00:00.000Z', last_active_at: null, workspace_count: 0, two_factor: false, deleted: false },
  { id: 'user-5', email: 'deleted-user-5@deleted.invalid', display_name: 'Deleted user', root: false, admin: false, suspended: true, deleted: true, created_at: '2026-10-04T10:00:00.000Z', last_active_at: null, workspace_count: 0, two_factor: false },
]

/** The viewer is the root user unless `viewer` says otherwise; self-hosted with a mail server unless `options` says otherwise. */
function renderPage(requests: string[] = [], viewer: { id: string; root: boolean } = { id: 'root-1', root: true }, options: Partial<AuthOptions> = {}) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    requests.push(`${request.method} ${url.pathname}${url.search}`)
    if (request.method === 'PUT' && url.pathname.endsWith('/email')) {
      const { email } = await request.json() as { email: string }
      requests.push(`email ${email}`)
      if (email === 'root@orbit.test') {
        return Response.json({ type: 'about:blank', title: 'Email taken', status: 409, code: 'email_taken', detail: 'Another account has this email.', instance: url.pathname, request_id: 'r' }, { status: 409, headers: { 'content-type': 'application/problem+json' } })
      }
      return new Response(null, { status: 204 })
    }
    if (request.method === 'POST' && url.pathname.endsWith('/two-factor/reset')) return new Response(null, { status: 204 })
    if (request.method === 'POST') {
      return options.billing_enabled
        ? Response.json({ sent_to: 'ada@orbit.test', expires_at: '2026-10-08T10:30:00.000Z' }, { status: 202 })
        : Response.json({ url: 'https://orbit.test/recovery?token=secret', expires_at: '2026-10-08T10:30:00.000Z' }, { status: 201 })
    }
    const q = url.searchParams.get('q')?.toLowerCase() ?? ''
    return Response.json({ items: USERS.filter((user) => `${user.display_name} ${user.email}`.toLowerCase().includes(q)), next_cursor: null })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.currentUser, { id: viewer.id, email: 'viewer@orbit.test', display_name: 'Viewer', installation_admin: true, root: viewer.root, status: { presence: 'online' } })
  client.setQueryData<AuthOptions>(queryKeys.authOptions, { registration_open: false, email_enabled: true, passkeys_enabled: false, billing_enabled: false, providers: [], provider_registration_open: false, ...options })
  return render(<QueryClientProvider client={client}><AdminUsersPage /><Toaster /></QueryClientProvider>)
}

test('the root user can only change their own email and a suspended account can only be reinstated or renamed', async () => {
  const view = renderPage()
  await view.findByText('Root Person')
  expect(view.getByText('Root')).toBeTruthy()
  expect(view.getByText('Suspended')).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Manage Root Person' }))
  await view.findByRole('menuitem', { name: 'Change email' })
  expect(view.getAllByRole('menuitem').map((item) => item.textContent)).toEqual(['Change email'])
  view.unmount()

  const again = renderPage()
  await again.findByText('Bob')
  fireEvent.click(again.getByRole('button', { name: 'Manage Bob' }))
  await again.findByRole('menuitem', { name: 'Reinstate account' })
  expect(again.getAllByRole('menuitem').map((item) => item.textContent)).toEqual(['Change email', 'Reinstate account'])
})

test('a deleted account shows only a Deleted badge, no email and no actions', async () => {
  const view = renderPage()
  const row = (await view.findByText('Deleted user')).closest('[data-admin-user]')!
  expect(row.getAttribute('data-admin-user')).toBe('user-5')
  expect(row.textContent).toContain('Deleted')
  expect(row.textContent).toContain('Anonymized. It can never sign in again.')
  expect(row.textContent).not.toContain('deleted-user-5@deleted.invalid')
  expect(row.textContent).not.toContain('Suspended')
  expect(view.queryAllByRole('button', { name: 'Manage Deleted user' })).toHaveLength(0)
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

test('only the root user manages admins; an admin cannot open the menu of another admin and only changes their own email', async () => {
  const requests: string[] = []
  const root = renderPage(requests)
  await root.findByText('Cy')
  expect(root.getByText('Admin')).toBeTruthy()
  fireEvent.click(root.getByRole('button', { name: 'Manage Ada' }))
  expect(await root.findByRole('menuitem', { name: 'Make admin' })).toBeTruthy()
  root.unmount()

  const admin = renderPage([], { id: 'user-4', root: false })
  await admin.findByText('Ada')
  expect(admin.queryByRole('button', { name: 'Manage Root Person' })).toBeNull()
  fireEvent.click(admin.getByRole('button', { name: 'Manage Ada' }))
  expect(await admin.findByRole('menuitem', { name: 'Create password recovery link' })).toBeTruthy()
  expect(admin.queryAllByRole('menuitem', { name: 'Make admin' })).toHaveLength(0)
  admin.unmount()

  const own = renderPage([], { id: 'user-4', root: false })
  await own.findByText('Cy')
  fireEvent.click(own.getByRole('button', { name: 'Manage Cy' }))
  await own.findByRole('menuitem', { name: 'Change email' })
  expect(own.getAllByRole('menuitem').map((item) => item.textContent)).toEqual(['Change email'])
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

test('changing an email shows the current one, shows a taken address under the field and refreshes the list', async () => {
  const requests: string[] = []
  const view = renderPage(requests)
  await view.findByText('Ada')
  fireEvent.click(view.getByRole('button', { name: 'Manage Ada' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Change email' }))
  const field = await view.findByLabelText('New email')
  expect(view.getByText('Now ada@orbit.test.')).toBeTruthy()
  expect(view.getByText('The person is signed out everywhere. No confirmation email is sent.')).toBeTruthy()

  await userEvent.type(field, 'root@orbit.test')
  await userEvent.click(view.getByRole('button', { name: 'Change email' }))
  expect(await view.findByText('Another account has this email.')).toBeTruthy()

  await userEvent.type(field, '{Backspace}'.repeat('root@orbit.test'.length) + ' ada@new.test ')
  const listed = requests.filter((request) => request.startsWith('GET /api/v1/admin/users')).length
  await userEvent.click(view.getByRole('button', { name: 'Change email' }))
  await waitFor(() => expect(view.queryAllByLabelText('New email')).toHaveLength(0))
  expect(requests.filter((request) => request.startsWith('PUT') || request.startsWith('email'))).toEqual([
    'PUT /api/v1/admin/users/user-2/email', 'email root@orbit.test',
    'PUT /api/v1/admin/users/user-2/email', 'email ada@new.test',
  ])
  await waitFor(() => expect(requests.filter((request) => request.startsWith('GET /api/v1/admin/users')).length).toBeGreaterThan(listed))
})

test('on Orbit Cloud the password reset is emailed to the person and the link is never shown', async () => {
  const requests: string[] = []
  const view = renderPage(requests, undefined, { billing_enabled: true })
  await view.findByText('Ada')
  fireEvent.click(view.getByRole('button', { name: 'Manage Ada' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Send password reset email' }))
  expect(await view.findByText('Sent to ada@orbit.test')).toBeTruthy()
  expect(requests).toContain('POST /api/v1/admin/users/user-2/recovery-link')
  expect(view.queryAllByLabelText('Recovery link')).toHaveLength(0)
})

test('on Orbit Cloud without a mail server the actions that email the person are off and say why', async () => {
  const requests: string[] = []
  const view = renderPage(requests, undefined, { billing_enabled: true, email_enabled: false })
  await view.findByText('Ada')
  fireEvent.click(view.getByRole('button', { name: 'Manage Ada' }))
  await view.findByRole('menuitem', { name: /Change email/ })
  const items = view.getAllByRole('menuitem').map((item) => [item.textContent, item.getAttribute('aria-disabled')])
  expect(items).toEqual([
    ['Change emailNeeds a mail server on Orbit Cloud', 'true'],
    ['Make admin', null],
    ['Send password reset emailNeeds a mail server on Orbit Cloud', 'true'],
    ['Reset two-factorNeeds a mail server on Orbit Cloud', 'true'],
    ['Suspend account', null],
  ])
  fireEvent.click(view.getByRole('menuitem', { name: /Send password reset email/ }))
  expect(requests.filter((request) => request.startsWith('POST'))).toEqual([])
})
