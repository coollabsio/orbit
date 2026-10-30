import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { queryKeys } from '@/api/queryKeys'
import { ConfirmEmailPage } from './ConfirmEmailPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch; window.history.replaceState(null, '', '/') })

function problem(status: number, code: string, detail: string) {
  return Response.json({ type: 'about:blank', title: 'Request failed', status, code, detail, instance: '/api/v1/auth/email-change/confirm', request_id: 'req-1' }, { status, headers: { 'content-type': 'application/problem+json' } })
}

function setup(answer: () => Response) {
  window.history.replaceState(null, '', '/confirm-email?token=change-token')
  const calls: { method: string; path: string; body: unknown }[] = []
  globalThis.fetch = (async (request: Request) => {
    const path = new URL(request.url).pathname
    calls.push({ method: request.method, path, body: await request.json().catch(() => undefined) })
    if (path === '/api/v1/auth/email-change/confirm') return answer()
    throw new Error(`Unexpected ${request.method} ${path}`)
  }) as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  client.setQueryData(queryKeys.currentUser, { id: 'user-1', email: 'old@orbit.test', display_name: 'Owner' })
  const view = render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/confirm-email']}><ConfirmEmailPage /></MemoryRouter></QueryClientProvider>)
  return { view, calls, client }
}

test('the link posts its token once and shows the new email', async () => {
  const { view, calls, client } = setup(() => Response.json({ email: 'new@orbit.test' }))
  expect(await view.findByText('Your email is changed')).toBeTruthy()
  expect(view.getByText('You now sign in with new@orbit.test. Your other devices are signed out.')).toBeTruthy()
  expect(view.getByRole('link', { name: 'Continue to Orbit' }).getAttribute('href')).toBe('/')
  expect(calls).toEqual([{ method: 'POST', path: '/api/v1/auth/email-change/confirm', body: { token: 'change-token' } }])
  expect(window.location.search).toBe('')
  expect(client.getQueryState(queryKeys.currentUser)?.isInvalidated).toBe(true)
})

test('a used or expired link says so', async () => {
  const { view } = setup(() => problem(400, 'invalid_email_change_token', 'This link is invalid, used or expired. Ask for a new one in your account settings.'))
  expect(await view.findByText('This link does not work')).toBeTruthy()
  expect(view.getByRole('alert').textContent).toBe('This link is invalid, used or expired. Ask for a new one in your account settings.')
})

test('an address another account took in the meantime says so', async () => {
  const { view } = setup(() => problem(409, 'email_taken', 'Another account has this email now. Choose another address.'))
  expect(await view.findByText('This email is taken')).toBeTruthy()
  expect(view.getByRole('alert').textContent).toBe('Another account has this email now. Choose another address.')
})
