import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { queryKeys } from '@/api/queryKeys'
import { clearProviderFlow, saveProviderFlow, storedProviderFlow } from '@/features/auth/providerSignIn'
import { ProviderCallbackPage } from './ProviderCallbackPage'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
  clearProviderFlow()
})

const USER = { id: 'user-1', email: 'ada@orbit.test', display_name: 'Ada', installation_admin: false, root: false, has_password: false, status: { presence: 'online' } }
const VERIFIER = 'v'.repeat(43)

function problem(status: number, code: string, title: string, detail: string) {
  return Response.json({ type: 'about:blank', title, status, code, detail, instance: '/', request_id: 'test' }, {
    status, headers: { 'content-type': 'application/problem+json' },
  })
}

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname + location.search}</output>
}

/** `exchange` answers the POST to /providers/exchange; with `started`, this tab began a Google sign-in. */
function setup(exchange: () => Response, hash = '#handoff=handoff-1', started = true) {
  if (started) saveProviderFlow({ provider: 'google', intent: 'sign_in', verifier: VERIFIER })
  const bodies: { path: string; body: unknown }[] = []
  globalThis.fetch = (async (request: Request) => {
    const path = new URL(request.url).pathname
    bodies.push({ path, body: await request.json() })
    if (path === '/api/v1/auth/providers/exchange') return exchange()
    if (path === '/api/v1/auth/login/two-factor') return Response.json({ user: USER, session_id: 'session-1' })
    throw new Error('Unexpected request ' + path)
  }) as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[`/login/callback${hash}`]}>
        <Routes>
          <Route path="/login/callback" element={<ProviderCallbackPage />} />
          <Route path="*" element={null} />
        </Routes>
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { view, bodies, client }
}

test('a sign-in trades the handoff and the verifier for a session and goes to the return path', async () => {
  const { view, bodies, client } = setup(() => Response.json({ user: USER, session_id: 'session-1' }), '#handoff=handoff-1&return_to=%2Foauth%2Fconsent%3Frequest%3D' + 'r'.repeat(43))
  await view.findByText('/oauth/consent?request=' + 'r'.repeat(43), { selector: 'output' })
  expect(bodies).toEqual([{ path: '/api/v1/auth/providers/exchange', body: { handoff: 'handoff-1', code_verifier: VERIFIER } }])
  expect(client.getQueryData<typeof USER>(queryKeys.currentUser)).toEqual(USER)
  expect(storedProviderFlow()).toBe(null)
})

test('an account with two-factor sign-in continues with the code step', async () => {
  const { view, bodies } = setup(() => Response.json({ two_factor_token: 'challenge-1' }, { status: 202 }))
  await userEvent.type(await view.findByLabelText('Authentication code'), '123456')
  await userEvent.click(view.getByRole('button', { name: 'Verify' }))
  await view.findByText('/', { selector: 'output' })
  expect(bodies.at(-1)).toEqual({ path: '/api/v1/auth/login/two-factor', body: { two_factor_token: 'challenge-1', code: '123456' } })
})

test('a refused sign-in says why and links back to sign in', async () => {
  const { view } = setup(() => problem(403, 'registration_closed', 'Registration closed', 'There is no Orbit account for this email, and this server does not accept new accounts.'))
  expect((await view.findByRole('alert')).textContent).toBe('There is no Orbit account for this email, and this server does not accept new accounts.')
  expect(view.getByRole('heading', { name: 'Registration closed' })).toBeTruthy()
  expect(view.getByRole('link', { name: 'Back to sign in' }).getAttribute('href')).toBe('/login')
  expect(view.getByTestId('location').textContent).toBe('/login/callback')
})

test('a callback this tab did not start does not call the server', async () => {
  const { view, bodies } = setup(() => Response.json({ user: USER, session_id: 'session-1' }), '#handoff=handoff-1', false)
  expect((await view.findByRole('alert')).textContent).toBe('This sign-in has expired or was opened in another tab. Start again.')
  expect(bodies).toHaveLength(0)
})
