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

function serveFinish(reply: Response, seen: { path?: string; body?: unknown }) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'POST') {
      seen.path = new URL(request.url).pathname
      seen.body = await request.json()
      return reply
    }
    return Response.json({ status: 'disconnected', login: null })
  }) as typeof fetch
}

const problem = (status: number, code: string) => new Response(JSON.stringify({ type: 'about:blank', title: 'x', status, code, detail: 'x', instance: '/', request_id: 'r' }), { status, headers: { 'content-type': 'application/problem+json' } })

test('back from GitHub, the page finishes the connection as the signed-in member', async () => {
  const seen: { path?: string; body?: unknown } = {}
  serveFinish(Response.json({ status: 'connected', login: 'octo' }), seen)
  const view = renderWithWorkspace(<GithubAccountPage />, { path: '/profile/github?github=finish&token=t1' })
  expect(await view.findByText('GitHub connected')).toBeTruthy()
  expect(seen.path).toBe('/api/v1/workspaces/workspace-1/github/me/finish')
  expect(seen.body).toEqual({ token: 't1' })
})

test('the connection finishes in the workspace the flow started in', async () => {
  const seen: { path?: string; body?: unknown } = {}
  serveFinish(Response.json({ status: 'connected', login: 'octo' }), seen)
  const view = renderWithWorkspace(<GithubAccountPage />, { path: '/profile/github?github=finish&token=t1&workspace=workspace-2' })
  expect(await view.findByText('GitHub connected')).toBeTruthy()
  expect(seen.path).toBe('/api/v1/workspaces/workspace-2/github/me/finish')
})

test('a GitHub account of another member is refused with a clear message', async () => {
  serveFinish(problem(409, 'github_account_taken'), {})
  const view = renderWithWorkspace(<GithubAccountPage />, { path: '/profile/github?github=finish&token=t1' })
  expect(await view.findByText('This GitHub account is already connected to another Orbit user.')).toBeTruthy()
})

test('a link that another member started is refused', async () => {
  serveFinish(problem(403, 'github_connect_mismatch'), {})
  const view = renderWithWorkspace(<GithubAccountPage />, { path: '/profile/github?github=finish&token=t1' })
  expect(await view.findByText('Another member started this GitHub connection. Connect from your own Account settings.')).toBeTruthy()
})

test('a failed GitHub flow shows once as a toast', async () => {
  serve('disconnected')
  const view = renderWithWorkspace(<GithubAccountPage />, { path: '/profile/github?github=failed' })
  expect(await view.findByText('Could not connect GitHub. Try again.')).toBeTruthy()
})

test('a broken connection asks to reconnect', async () => {
  serve('broken', 'octo')
  const view = renderWithWorkspace(<GithubAccountPage />, { path: '/profile/github' })
  expect(await view.findByRole('button', { name: 'Reconnect GitHub' })).toBeTruthy()
})
