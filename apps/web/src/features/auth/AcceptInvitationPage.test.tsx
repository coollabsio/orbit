import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, useLocation } from 'react-router'
import { AcceptInvitationPage } from './AuthGate'
import { UNAUTHORIZED_EVENT } from '../../api/client'

function problem(status: number, code: string, detail: string) {
  return Response.json({ type: 'about:blank', title: 'Request failed', status, code, detail, instance: '/', request_id: 'test' }, { status })
}

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch; window.history.replaceState(null, '', '/') })

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

function setup(session: { email: string; display_name: string; id: string } | null = null, invalid = false) {
  window.history.replaceState(null, '', '/accept-invitation?token=secret-invite')
  const bodies: { path: string; body: Record<string, unknown> }[] = []
  globalThis.fetch = (async (request: Request) => {
    const path = new URL(request.url).pathname
    const body = request.body ? await request.json() : {}
    bodies.push({ path, body })
    if (path.endsWith('/preview')) return invalid
      ? problem(404, 'invitation_not_found', 'Invalid invitation')
      : Response.json({ email: 'invited@example.com', workspace_name: 'Alpha' })
    if (path.endsWith('/me')) return session
      ? Response.json(session)
      : problem(401, 'authentication_required', 'Sign in required')
    if (path.endsWith('/logout')) { session = null; return new Response(null, { status: 204 }) }
    if (path.endsWith('/login')) {
      session = { id: 'new-user', display_name: 'Invited', email: 'invited@example.com' }
      return Response.json({ user: session })
    }
    if (path.endsWith('/accept')) {
      session = { id: 'new-user', display_name: 'Invited', email: 'invited@example.com' }
      return Response.json({ workspace_id: 'alpha', membership_id: 'member', created: true })
    }
    throw new Error('Unexpected request ' + path)
  }) as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/accept-invitation']}>
    <AcceptInvitationPage /><Location />
  </MemoryRouter></QueryClientProvider>)
  return { view, bodies, client }
}

test('invitation loads a read-only email and registers with the server-provided identity', async () => {
  let unauthorized = 0
  const listener = () => { unauthorized++ }
  window.addEventListener(UNAUTHORIZED_EVENT, listener)
  try {
    const { view, bodies, client } = setup()
    const email = await view.findByLabelText('Email') as HTMLInputElement
    expect(email.value).toBe('invited@example.com')
    expect(email.readOnly).toBe(true)
    expect(window.location.search).toBe('')
    await userEvent.type(view.getByLabelText('Your name'), 'Invited')
    await userEvent.type(view.getByLabelText('Password'), 'a-secure-password')
    fireEvent.submit(email.closest('form')!)
    await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/?workspace=alpha'))
    expect(bodies.find((call) => call.path.endsWith('/accept'))?.body).toEqual({
      token: 'secret-invite', email: 'invited@example.com', display_name: 'Invited', password: 'a-secure-password',
    })
    expect(client.getQueryData(['current-user'])).toMatchObject({ email: 'invited@example.com' })
    expect(unauthorized).toBe(0)
  } finally { window.removeEventListener(UNAUTHORIZED_EVENT, listener) }
})

test('existing users can sign in without losing their invitation', async () => {
  const { view, bodies } = setup()
  fireEvent.click(await view.findByRole('button', { name: 'Sign in instead' }))
  expect(view.queryByLabelText('Your name')).toBeNull()
  await userEvent.type(view.getByLabelText('Password'), 'existing-password')
  fireEvent.click(view.getByRole('button', { name: 'Sign in and accept invitation' }))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/?workspace=alpha'))
  expect(bodies.find((call) => call.path.endsWith('/login'))?.body).toEqual({ email: 'invited@example.com', password: 'existing-password' })
  expect(bodies.find((call) => call.path.endsWith('/accept'))?.body).toEqual({ token: 'secret-invite' })
})

test('matching signed-in users accept without registration fields', async () => {
  const { view, bodies } = setup({ id: 'existing', display_name: 'Invited', email: 'Invited@Example.com' })
  fireEvent.click(await view.findByRole('button', { name: 'Accept invitation' }))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/?workspace=alpha'))
  expect(bodies.find((call) => call.path.endsWith('/accept'))?.body).toEqual({ token: 'secret-invite' })
  expect(view.queryByLabelText('Password')).toBeNull()
})

test('a mismatched session must switch accounts and keeps the invitation', async () => {
  const { view, bodies } = setup({ id: 'other', display_name: 'Other', email: 'other@example.com' })
  const switchButton = await view.findByRole('button', { name: 'Switch account' })
  expect(view.queryByRole('button', { name: 'Accept invitation' })).toBeNull()
  fireEvent.click(switchButton)
  expect(await view.findByRole('button', { name: 'Sign in and accept invitation' })).toBeTruthy()
  expect((view.getByLabelText('Email') as HTMLInputElement).value).toBe('invited@example.com')
  expect(bodies.filter((call) => call.path.endsWith('/accept'))).toHaveLength(0)
  expect(view.getByTestId('location').textContent).toBe('/accept-invitation')
})

test('invalid invitations never display an account creation form', async () => {
  const { view } = setup(null, true)
  expect(await view.findByRole('heading', { name: 'Invitation unavailable' })).toBeTruthy()
  expect(view.queryByLabelText('Password')).toBeNull()
})

test('failed acceptance after sign-in can retry without submitting credentials again', async () => {
  const { view, bodies } = setup()
  const server = globalThis.fetch
  let attempts = 0
  globalThis.fetch = (async (request: Request) => {
    if (request.url.endsWith('/accept') && ++attempts === 1) {
      return problem(503, 'unavailable', 'Please retry.')
    }
    return server(request)
  }) as typeof fetch
  fireEvent.click(await view.findByRole('button', { name: 'Sign in instead' }))
  await userEvent.type(view.getByLabelText('Password'), 'existing-password')
  fireEvent.click(view.getByRole('button', { name: 'Sign in and accept invitation' }))
  expect((await view.findByRole('alert')).textContent).toContain('Please retry.')
  expect(view.queryByLabelText('Password')).toBeNull()
  fireEvent.click(view.getByRole('button', { name: 'Accept invitation' }))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/?workspace=alpha'))
  expect(attempts).toBe(2)
  expect(bodies.filter((call) => call.path.endsWith('/login'))).toHaveLength(1)
  expect(bodies.find((call) => call.path.endsWith('/accept'))?.body).toEqual({ token: 'secret-invite' })
})

test('failed sign-in stays on the invitation and allows correcting the password', async () => {
  const { view, bodies } = setup()
  const server = globalThis.fetch
  let attempts = 0
  globalThis.fetch = (async (request: Request) => {
    if (request.url.endsWith('/login') && ++attempts === 1) {
      return problem(401, 'invalid_credentials', 'Incorrect password.')
    }
    return server(request)
  }) as typeof fetch
  fireEvent.click(await view.findByRole('button', { name: 'Sign in instead' }))
  await userEvent.type(view.getByLabelText('Password'), 'wrong-password')
  fireEvent.click(view.getByRole('button', { name: 'Sign in and accept invitation' }))
  expect((await view.findByRole('alert')).textContent).toContain('Incorrect password.')
  expect(bodies.filter((call) => call.path.endsWith('/accept'))).toHaveLength(0)
  await userEvent.clear(view.getByLabelText('Password'))
  await userEvent.type(view.getByLabelText('Password'), 'correct-password')
  fireEvent.click(view.getByRole('button', { name: 'Sign in and accept invitation' }))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/?workspace=alpha'))
  expect(attempts).toBe(2)
})
