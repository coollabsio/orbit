import { afterEach, expect, test } from 'bun:test'
import { waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { registerConfirmationHandler } from '@/components/common/confirmAction'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { GithubAccountPage } from './GithubAccountPage'

const originalFetch = globalThis.fetch
const originalAssign = window.location.assign
afterEach(() => { globalThis.fetch = originalFetch; window.location.assign = originalAssign })

function serve(status: string, login: string | null = null, onRequest?: (request: Request) => void) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    onRequest?.(request)
    if (request.method === 'POST') return Response.json({ authorize_url: 'https://github.com/login/oauth/authorize?client_id=Iv1.abc' })
    if (request.method === 'DELETE') return new Response(null, { status: 204 })
    return Response.json({ status, login })
  }) as typeof fetch
}

test('a connected account shows its login and can be disconnected', async () => {
  const methods: string[] = []
  serve('connected', 'octo', (request) => methods.push(request.method))
  const unregister = registerConfirmationHandler(async () => true)
  const view = renderWithWorkspace(<GithubAccountPage />, { path: '/profile/github' })
  expect(await view.findByText('octo')).toBeTruthy()
  await userEvent.click(view.getByRole('button', { name: 'Disconnect GitHub' }))
  await waitFor(() => expect(methods).toContain('DELETE'))
  unregister()
})

test('connect starts the web flow and opens GitHub', async () => {
  let body: unknown
  let path = ''
  serve('disconnected', null, (request) => {
    if (request.method === 'POST') { path = new URL(request.url).pathname; void request.clone().json().then((value) => { body = value }) }
  })
  let opened = ''
  window.location.assign = ((url: string) => { opened = url }) as typeof window.location.assign
  const view = renderWithWorkspace(<GithubAccountPage />, { path: '/profile/github' })
  await userEvent.click(await view.findByRole('button', { name: 'Connect GitHub' }))
  await waitFor(() => expect(opened).toBe('https://github.com/login/oauth/authorize?client_id=Iv1.abc'))
  expect(path).toBe('/api/v1/workspaces/workspace-1/github/me')
  expect(body).toEqual({ client: 'web' })
})

test('the result of the GitHub flow shows once as a toast', async () => {
  serve('disconnected')
  const view = renderWithWorkspace(<GithubAccountPage />, { path: '/profile/github?github=taken' })
  expect(await view.findByText('This GitHub account is already connected to another Orbit user.')).toBeTruthy()
})

test('a broken connection asks to reconnect', async () => {
  serve('broken', 'octo')
  const view = renderWithWorkspace(<GithubAccountPage />, { path: '/profile/github' })
  expect(await view.findByRole('button', { name: 'Reconnect GitHub' })).toBeTruthy()
})
