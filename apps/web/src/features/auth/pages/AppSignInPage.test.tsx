import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { AppSignInPage } from './AppSignInPage'

const scope = globalThis as { PublicKeyCredential?: unknown }
const originalFetch = globalThis.fetch
const originalCredential = scope.PublicKeyCredential
const originalCredentials = Object.getOwnPropertyDescriptor(navigator, 'credentials')
const originalAssign = Object.getOwnPropertyDescriptor(window.location, 'assign')

afterEach(() => {
  globalThis.fetch = originalFetch
  scope.PublicKeyCredential = originalCredential
  if (originalCredentials) Object.defineProperty(navigator, 'credentials', originalCredentials)
  else delete (navigator as { credentials?: unknown }).credentials
  if (originalAssign) Object.defineProperty(window.location, 'assign', originalAssign)
})

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname + location.search + location.hash}</output>
}

/** A browser with one passkey; `finish` answers the page's POST to /passkey/app/finish. */
function setup(hash: string, finish: () => Response) {
  scope.PublicKeyCredential = { parseCreationOptionsFromJSON: (json: unknown) => json, parseRequestOptionsFromJSON: (json: unknown) => json }
  Object.defineProperty(navigator, 'credentials', { configurable: true, value: { get: async () => ({ toJSON: () => ({ id: 'passkey-1' }) }) } })
  const left: string[] = []
  Object.defineProperty(window.location, 'assign', { configurable: true, value: (url: string) => { left.push(url) } })
  const bodies: { path: string; body: unknown }[] = []
  globalThis.fetch = (async (request: Request) => {
    const path = new URL(request.url).pathname
    bodies.push({ path, body: await request.json().catch(() => null) })
    if (path === '/api/v1/auth/passkey/login/start') return Response.json({ challenge_token: 'challenge-1', options: { publicKey: { challenge: 'c' } } })
    if (path === '/api/v1/auth/passkey/app/finish') return finish()
    throw new Error('Unexpected request ' + path)
  }) as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[`/login/app${hash}`]}>
        <Routes><Route path="/login/app" element={<><AppSignInPage /><Location /></>} /></Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { view, bodies, left }
}

test('asks for the passkey only after a click, then sends the browser to the app with the handoff', async () => {
  const redirect = 'orbit://auth/callback?handoff=handoff-1'
  const { view, bodies, left } = setup('#flow=flow-1', () => Response.json({ redirect_url: redirect }))
  await view.findByText('Sign in to the Orbit app')
  // The flow token leaves the address, and nothing runs before the person acts.
  expect(view.getByTestId('location').textContent).toBe('/login/app')
  expect(bodies).toEqual([])

  await userEvent.click(view.getByRole('button', { name: 'Sign in with a passkey' }))
  const link = await view.findByRole('link', { name: 'Open the Orbit app' })
  expect(link.getAttribute('href')).toBe(redirect)
  expect(left).toEqual([redirect])
  expect(bodies).toEqual([
    { path: '/api/v1/auth/passkey/login/start', body: null },
    { path: '/api/v1/auth/passkey/app/finish', body: { flow: 'flow-1', challenge_token: 'challenge-1', credential: { id: 'passkey-1' } } },
  ])
})

test('a used or expired flow sends the person back to the app', async () => {
  const { view, left } = setup('#flow=flow-1', () => Response.json(
    { type: 'about:blank', title: 'Sign-in expired', status: 400, code: 'invalid_app_sign_in', detail: 'Expired.', instance: '/', request_id: 'test' },
    { status: 400, headers: { 'content-type': 'application/problem+json' } },
  ))
  await userEvent.click(await view.findByRole('button', { name: 'Sign in with a passkey' }))
  await view.findByText('This sign-in has expired or was already used. Go back to the Orbit app and start again.')
  expect(view.queryByRole('button')).toBeNull()
  expect(left).toEqual([])
})

test('a page opened without a flow offers no sign-in', async () => {
  const { view, bodies } = setup('', () => { throw new Error('not reached') })
  await view.findByText('Sign-in expired')
  expect(view.queryByRole('button')).toBeNull()
  expect(bodies).toEqual([])
})
