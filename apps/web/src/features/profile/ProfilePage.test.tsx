import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { queryKeys } from '../../api/queryKeys'
import { ProfilePage } from './ProfilePage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const user = { id: 'user-1', email: 'owner@orbit.test', display_name: 'Owner' }

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

function setup(handler?: (request: Request, body: unknown) => Response | Promise<Response>) {
  const calls: { method: string; path: string; body: unknown }[] = []
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const path = new URL(request.url, 'http://orbit.test').pathname
    const body = ['GET', 'HEAD'].includes(request.method) ? undefined : await request.json().catch(() => undefined)
    calls.push({ method: request.method, path, body })
    if (handler) return handler(request, body)
    if (path === '/api/v1/auth/me' && request.method === 'PATCH') {
      return Response.json({ ...user, display_name: (body as { display_name: string }).display_name })
    }
    if (path === '/api/v1/auth/password') return new Response(null, { status: 204 })
    throw new Error(`Unexpected ${request.method} ${path}`)
  }) as typeof fetch
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } },
  })
  client.setQueryData(queryKeys.currentUser, user)
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  )
  return { view: render(<ProfilePage />, { wrapper }), calls, client }
}

test('profile shows a read-only email and omits avatar, email-change, and 2FA', () => {
  const { view } = setup()
  const email = view.getByLabelText('Email') as HTMLInputElement
  expect(email.value).toBe('owner@orbit.test')
  expect(email.readOnly).toBe(true)
  expect((view.getByLabelText(/^Name/) as HTMLInputElement).value).toBe('Owner')
  expect(view.queryByRole('button', { name: 'Change' })).toBeNull()
  expect(view.queryByText(/profile picture/i)).toBeNull()
  expect(view.queryByText(/two-factor/i)).toBeNull()
  expect(view.queryByRole('button', { name: /2fa/i })).toBeNull()
  expect(view.getByText('Other devices will be signed out.')).toBeTruthy()
})

test('name edits use the shared save popup and Reset restores the saved name', async () => {
  const { view } = setup()
  expect(view.queryByRole('button', { name: 'Save' })).toBeNull()
  expect(view.queryByRole('button', { name: 'Save Changes' })).toBeNull()
  const name = view.getByLabelText(/^Name/) as HTMLInputElement
  await userEvent.clear(name)
  await userEvent.type(name, 'Ada Lovelace')
  expect(view.getByRole('button', { name: 'Save Changes' })).toBeTruthy()
  expect(view.getByText('Careful — you have unsaved changes!')).toBeTruthy()
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
    throw new Error(`Unexpected ${request.method} ${request.url}`)
  })
  await userEvent.type(view.getByLabelText(/^Current password/), 'wrong password value')
  await userEvent.type(view.getByLabelText(/^New password/), 'brand new horse battery')
  await userEvent.type(view.getByLabelText(/^Confirm new password/), 'brand new horse battery')
  fireEvent.submit(view.getByLabelText(/^Current password/).closest('form')!)
  expect((await view.findByRole('alert')).textContent).toContain('Current password is incorrect.')
  expect((view.getByLabelText(/^New password/) as HTMLInputElement).value).toBe('brand new horse battery')
  expect((view.getByLabelText(/^Confirm new password/) as HTMLInputElement).value).toBe('brand new horse battery')
})
