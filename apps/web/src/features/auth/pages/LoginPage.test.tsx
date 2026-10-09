import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, useLocation } from 'react-router'
import { queryKeys } from '@/api/queryKeys'
import { waitForAbsence } from '@/test/waitForAbsence'
import { LoginPage } from './LoginPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const USER = { id: 'user-1', email: 'ada@orbit.test', display_name: 'Ada', installation_admin: false, root: false, status: { presence: 'online' } }

function problem(status: number, code: string, detail: string) {
  return Response.json({ type: 'about:blank', title: 'Request failed', status, code, detail, instance: '/', request_id: 'test' }, {
    status, headers: { 'content-type': 'application/problem+json' },
  })
}

function Location() {
  return <output data-testid="location">{useLocation().pathname}</output>
}

/** `secondFactor` answers each POST to /login/two-factor with the code that was sent. */
function setup(secondFactor: (code: string) => Response) {
  const bodies: { path: string; body: Record<string, unknown> }[] = []
  globalThis.fetch = (async (request: Request) => {
    const path = new URL(request.url).pathname
    if (path.endsWith('/auth/options')) return Response.json({ registration_open: false, email_enabled: false, passkeys_enabled: false })
    const body = await request.json() as Record<string, unknown>
    bodies.push({ path, body })
    if (path.endsWith('/auth/login')) return Response.json({ two_factor_token: 'challenge-1' }, { status: 202 })
    if (path.endsWith('/auth/login/two-factor')) return secondFactor(String(body.code))
    throw new Error('Unexpected request ' + path)
  }) as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/login']}><LoginPage /><Location /></MemoryRouter>
    </QueryClientProvider>,
  )
  return { view, bodies, client }
}

async function fill(input: HTMLElement, text: string) {
  const value = (input as HTMLInputElement).value
  if (value) await userEvent.type(input, '{Backspace}'.repeat(value.length))
  await userEvent.type(input, text)
}

async function signInWithPassword(view: ReturnType<typeof render>) {
  await fill(view.getByLabelText('Email'), 'ada@orbit.test')
  await fill(view.getByLabelText('Password'), 'secret')
  await userEvent.click(view.getByRole('button', { name: 'Sign in' }))
  return view.findByLabelText('Authentication code')
}

test('a two-factor account asks for a code after the password and signs in with it', async () => {
  const { view, bodies, client } = setup((code) => code === '123456'
    ? Response.json({ user: USER, session_id: 'session-1' })
    : problem(401, 'invalid_two_factor_code', 'The code is not right.'))

  const code = await signInWithPassword(view)
  expect(view.getByText('Enter the 6-digit code from your authenticator app, or a recovery code.')).toBeTruthy()

  await fill(code, '000000')
  await userEvent.click(view.getByRole('button', { name: 'Verify' }))
  expect((await view.findByRole('alert')).textContent).toBe('The code is not right.')
  expect(view.getByTestId('location').textContent).toBe('/login')

  await fill(code, '123456')
  await userEvent.click(view.getByRole('button', { name: 'Verify' }))
  await view.findByText('/', { selector: 'output' })
  expect(client.getQueryData<typeof USER>(queryKeys.currentUser)).toEqual(USER)
  expect(bodies.at(-1)).toEqual({ path: '/api/v1/auth/login/two-factor', body: { two_factor_token: 'challenge-1', code: '123456' } })
})

test('an expired challenge goes back to the password step with the reason', async () => {
  const { view } = setup(() => problem(400, 'invalid_two_factor_token', 'Sign in again: the code step expired.'))
  const code = await signInWithPassword(view)
  await fill(code, 'k3x9p-7mq2d')
  await userEvent.click(view.getByRole('button', { name: 'Verify' }))

  expect((await view.findByRole('alert')).textContent).toBe('Sign in again: the code step expired.')
  await waitForAbsence(() => view.queryByLabelText('Authentication code'))
  expect(view.getByLabelText('Password')).toBeTruthy()
})

test('Back returns to the password step', async () => {
  const { view } = setup(() => problem(401, 'invalid_two_factor_code', 'unused'))
  await signInWithPassword(view)
  await userEvent.click(view.getByRole('button', { name: 'Back' }))
  expect(await view.findByRole('button', { name: 'Sign in' })).toBeTruthy()
  expect(view.queryAllByLabelText('Authentication code')).toHaveLength(0)
})
