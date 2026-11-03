import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { queryKeys } from '@/api/queryKeys'
import { ProfilePage } from './ProfilePage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const user = { id: 'user-1', email: 'owner@orbit.test', display_name: 'Owner', has_password: true, root: false }
const OPTIONS = { email_enabled: true, billing_enabled: false, passkeys_enabled: false, providers: [], registration_open: false, provider_registration_open: false }

function problem(status: number, code: string, detail: string) {
  return Response.json({
    type: `https://docs.orbit.dev/problems/${code}`,
    title: 'Request failed',
    status,
    code,
    detail,
    instance: '/api/v1/auth/password',
    request_id: 'req-1',
  }, { status, headers: { 'content-type': 'application/problem+json' } })
}

type Handler = (request: Request, body: unknown) => Response | Promise<Response> | undefined

/** `handler` answers first; `undefined` falls through to the defaults. */
function setup(handler?: Handler, { me = user, options = OPTIONS }: { me?: Record<string, unknown>; options?: Record<string, unknown> } = {}) {
  const calls: { method: string; path: string; body: unknown }[] = []
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const path = new URL(request.url, 'http://orbit.test').pathname
    const body = ['GET', 'HEAD'].includes(request.method) ? undefined : await request.json().catch(() => undefined)
    calls.push({ method: request.method, path, body })
    const handled = await handler?.(request, body)
    if (handled) return handled
    if (path === '/api/v1/auth/options') return Response.json(options)
    if (path === '/api/v1/auth/me' && request.method === 'PATCH') {
      return Response.json({ ...user, display_name: (body as { display_name: string }).display_name })
    }
    if (path === '/api/v1/auth/password') return new Response(null, { status: 204 })
    throw new Error(`Unexpected ${request.method} ${path}`)
  }) as typeof fetch
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } },
  })
  client.setQueryData(queryKeys.currentUser, me)
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/profile']}>
        <Routes>
          <Route path="/profile" element={children} />
          <Route path="/login" element={<p>Sign-in page</p>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>
  )
  return { view: render(<ProfilePage />, { wrapper }), calls, client }
}

test('profile shows the email read-only with a Change button, and no 2FA', async () => {
  const { view } = setup()
  const email = view.getByLabelText('Email') as HTMLInputElement
  expect(email.value).toBe('owner@orbit.test')
  expect(email.readOnly).toBe(true)
  expect((view.getByLabelText(/^Name/) as HTMLInputElement).value).toBe('Owner')
  expect(await view.findByRole('button', { name: 'Change' })).toBeTruthy()
  expect(view.queryAllByText(/two-factor/i)).toHaveLength(0)
  expect(view.getByText('Other devices will be signed out.')).toBeTruthy()
})

test('without a mail server the email cannot be changed and the page says why', async () => {
  const { view } = setup(undefined, { options: { ...OPTIONS, email_enabled: false } })
  expect(await view.findByText('This Orbit cannot send email, so the email cannot be changed. Ask an administrator.')).toBeTruthy()
  expect(view.queryAllByRole('button', { name: 'Change' })).toHaveLength(0)
})

test('name edits use the shared save popup and Reset restores the saved name', async () => {
  const { view } = setup()
  expect(view.queryByRole('button', { name: 'Save' })).toBeNull()
  expect(view.queryByRole('button', { name: 'Save Changes' })).toBeNull()
  const name = view.getByLabelText(/^Name/) as HTMLInputElement
  await userEvent.clear(name)
  await userEvent.type(name, 'Ada Lovelace')
  expect(view.getByRole('button', { name: 'Save Changes' })).toBeTruthy()
  expect(view.getByText('You have unsaved changes.')).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Reset' }))
  expect(name.value).toBe('Owner')
  expect(view.queryByRole('button', { name: 'Save Changes' })).toBeNull()
})

test('saving the display name patches /api/v1/auth/me', async () => {
  const { view, calls, client } = setup()
  const name = view.getByLabelText(/^Name/) as HTMLInputElement
  await userEvent.clear(name)
  await userEvent.type(name, 'Ada Lovelace')
  fireEvent.click(view.getByRole('button', { name: 'Save Changes' }))
  await waitFor(() => expect(calls.some((call) => call.method === 'PATCH' && call.path === '/api/v1/auth/me')).toBe(true))
  expect(calls.find((call) => call.method === 'PATCH')?.body).toEqual({ display_name: 'Ada Lovelace' })
  expect(client.getQueryData(queryKeys.currentUser)).toMatchObject({ display_name: 'Ada Lovelace' })
})

test('changing the password posts to /api/v1/auth/password', async () => {
  const { view, calls } = setup()
  await userEvent.type(view.getByLabelText(/^Current password/), 'correct horse battery')
  await userEvent.type(view.getByLabelText(/^New password/), 'brand new horse battery')
  await userEvent.type(view.getByLabelText(/^Confirm new password/), 'brand new horse battery')
  fireEvent.submit(view.getByLabelText(/^Current password/).closest('form')!)
  await waitFor(() => expect(calls.some((call) => call.path === '/api/v1/auth/password')).toBe(true))
  expect(calls.find((call) => call.path === '/api/v1/auth/password')).toEqual({
    method: 'POST',
    path: '/api/v1/auth/password',
    body: { current_password: 'correct horse battery', new_password: 'brand new horse battery' },
  })
})

test('wrong current password keeps the new-password fields', async () => {
  const { view } = setup((request) => {
    if (new URL(request.url, 'http://orbit.test').pathname === '/api/v1/auth/password') {
      return problem(401, 'invalid_credentials', 'Current password is incorrect.')
    }
  })
  await userEvent.type(view.getByLabelText(/^Current password/), 'wrong password value')
  await userEvent.type(view.getByLabelText(/^New password/), 'brand new horse battery')
  await userEvent.type(view.getByLabelText(/^Confirm new password/), 'brand new horse battery')
  fireEvent.submit(view.getByLabelText(/^Current password/).closest('form')!)
  expect((await view.findByRole('alert')).textContent).toContain('Current password is incorrect.')
  expect((view.getByLabelText(/^New password/) as HTMLInputElement).value).toBe('brand new horse battery')
  expect((view.getByLabelText(/^Confirm new password/) as HTMLInputElement).value).toBe('brand new horse battery')
})

const path = (request: Request) => new URL(request.url, 'http://orbit.test').pathname

test('changing the email sends the new address and the password, then says to check the inbox', async () => {
  const { view, calls } = setup((request) => {
    if (path(request) === '/api/v1/auth/me/email') return Response.json({ detail: 'We sent a link.' }, { status: 202 })
  })
  fireEvent.click(await view.findByRole('button', { name: 'Change' }))
  await userEvent.type(await view.findByLabelText('New email'), ' new@orbit.test ')
  await userEvent.type(view.getByLabelText('Current password'), 'correct horse battery')
  fireEvent.click(view.getByRole('button', { name: 'Send link' }))
  expect(await view.findByText('Check your inbox')).toBeTruthy()
  expect(view.getByText('We sent a link to new@orbit.test. Open it within 60 minutes to finish the change. Until then, you sign in with owner@orbit.test.')).toBeTruthy()
  expect(calls.find((call) => call.path === '/api/v1/auth/me/email')).toEqual({
    method: 'POST',
    path: '/api/v1/auth/me/email',
    body: { email: 'new@orbit.test', password: 'correct horse battery' },
  })
})

test('an account without a password changes the email without one and confirms with its provider when asked', async () => {
  const { view, calls } = setup((request) => {
    if (path(request) === '/api/v1/auth/me/email') return problem(401, 'reauthentication_required', 'Sign in with your connected account again to confirm this change.')
    if (path(request) === '/api/v1/auth/connected-accounts') return Response.json([{ provider: 'google', email: 'owner@gmail.test', created_at: '2026-10-01T10:00:00.000Z' }])
  }, { me: { ...user, has_password: false } })
  fireEvent.click(await view.findByRole('button', { name: 'Change' }))
  await userEvent.type(await view.findByLabelText('New email'), 'new@orbit.test')
  expect(view.queryAllByLabelText('Current password')).toHaveLength(0)
  fireEvent.click(view.getByRole('button', { name: 'Send link' }))
  expect(await view.findByRole('button', { name: 'Confirm with Google' })).toBeTruthy()
  expect(calls.find((call) => call.path === '/api/v1/auth/me/email')?.body).toEqual({ email: 'new@orbit.test', password: null })
})

async function openDeleteDialog(view: ReturnType<typeof setup>['view']) {
  fireEvent.click(view.getByRole('button', { name: 'Delete account' }))
  return view.findByLabelText('Confirm your email')
}

test('deleting the account sends the typed email and the password, clears the cache and goes to sign-in', async () => {
  const { view, calls, client } = setup((request) => {
    if (path(request) === '/api/v1/auth/me' && request.method === 'DELETE') return new Response(null, { status: 204 })
    if (path(request) === '/api/v1/auth/me') return problem(401, 'authentication_required', 'Sign in required')
  })
  await userEvent.type(await openDeleteDialog(view), 'owner@orbit.test')
  await userEvent.type(view.getByLabelText('Current password'), 'correct horse battery')
  const submit = view.getAllByRole('button', { name: 'Delete account' }).at(-1)!
  fireEvent.click(submit)
  expect(await view.findByText('Sign-in page')).toBeTruthy()
  expect(calls.find((call) => call.method === 'DELETE')).toEqual({
    method: 'DELETE',
    path: '/api/v1/auth/me',
    body: { email: 'owner@orbit.test', password: 'correct horse battery' },
  })
  expect(client.getQueryData(queryKeys.currentUser)).toBeUndefined()
})

test('the delete button waits for the exact email', async () => {
  const { view } = setup()
  await userEvent.type(await openDeleteDialog(view), 'someone@orbit.test')
  await userEvent.type(view.getByLabelText('Current password'), 'correct horse battery')
  expect((view.getAllByRole('button', { name: 'Delete account' }).at(-1) as HTMLButtonElement).disabled).toBe(true)
})

test('an owner of workspaces is told which ones to delete first', async () => {
  const { view } = setup((request) => {
    if (path(request) === '/api/v1/auth/me' && request.method === 'DELETE') {
      return Response.json({
        type: 'about:blank', title: 'You own workspaces', status: 409, code: 'owns_workspaces', detail: 'Delete the workspaces you own first.',
        instance: '/api/v1/auth/me', request_id: 'req-1', workspaces: [{ id: 'w-1', name: 'Alpha' }, { id: 'w-2', name: 'Beta' }],
      }, { status: 409, headers: { 'content-type': 'application/problem+json' } })
    }
  }, { options: { ...OPTIONS, billing_enabled: true } })
  await userEvent.type(await openDeleteDialog(view), 'Owner@orbit.test')
  await userEvent.type(view.getByLabelText('Current password'), 'correct horse battery')
  fireEvent.click(view.getAllByRole('button', { name: 'Delete account' }).at(-1)!)
  const alpha = await view.findByRole('link', { name: 'Alpha' })
  expect(alpha.getAttribute('href')).toBe('/settings/danger-zone?workspace=w-1')
  expect(view.getByRole('link', { name: 'Beta' })).toBeTruthy()
  expect(view.getByText('Delete them first in Workspace settings → Danger zone. Ownership transfer is not available on Orbit Cloud.')).toBeTruthy()
})

test('the root account has no delete button', () => {
  const { view } = setup(undefined, { me: { ...user, root: true } })
  expect(view.getByText('The root account cannot be deleted.')).toBeTruthy()
  expect(view.queryAllByRole('button', { name: 'Delete account' })).toHaveLength(0)
})
