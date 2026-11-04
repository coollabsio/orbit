import { afterEach, expect, test } from 'bun:test'
import { useEffect } from 'react'
import { render, waitFor } from '@testing-library/react'
import { Route, Routes, useLocation } from 'react-router'
import { Providers } from '@/app/Providers'
import { OAuthConsentPage } from './OAuthConsentPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch; window.history.replaceState(null, '', '/') })

test('anonymous OAuth consent keeps its return path despite the global session handler', async () => {
  const request = 'a'.repeat(43)
  window.history.replaceState(null, '', `/oauth/consent?request=${request}`)
  globalThis.fetch = Object.assign(async () => Response.json({ type: 'about:blank', title: 'Sign in', code: 'authentication_required', status: 401, detail: 'A valid session is required.', instance: '/api/v1/auth/me', request_id: 'test' }, { status: 401, headers: { 'content-type': 'application/problem+json' } }), { preconnect: originalFetch.preconnect })
  let destination: { path: string; search: string } | undefined
  function LocationState() {
    const location = useLocation()
    useEffect(() => { destination = { path: location.pathname, search: location.search } }, [location])
    return null
  }
  render(<Providers><LocationState /><Routes>
    <Route path="/oauth/consent" element={<OAuthConsentPage />} />
    <Route path="/login" element={null} />
  </Routes></Providers>)
  await waitFor(() => expect(destination?.path).toBe('/login'))
  expect(new URLSearchParams(destination?.search).get('return_to')).toBe(`/oauth/consent?request=${request}`)
})

for (const approved of [true, false]) {
  test(`remote consent ${approved ? 'approval' : 'denial'} preserves the callback`, async () => {
    const { fireEvent } = await import('@testing-library/react')
    const userEvent = (await import('@testing-library/user-event')).default.setup()
    const callback = `http://localhost:9999/callback?${approved ? 'code=secret' : 'error=access_denied'}&state=original`
    let body: unknown
    window.history.replaceState(null, '', `/oauth/consent?request=${'r'.repeat(43)}`)
    globalThis.fetch = Object.assign(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = new URL(input instanceof Request ? input.url : String(input), window.location.origin).pathname
      if (path === '/api/v1/auth/me') return Response.json({ id: 'user-1', display_name: 'Ada' })
      if (init?.method === 'POST') {
        body = JSON.parse(init.body as string)
        return Response.json({ redirect_uri: callback })
      }
      return Response.json({ client_name: 'Remote client', redirect_to: '127.0.0.1', csrf_token: 'nonce', scope: 'tasks:read', projects: [{ id: 'project-1', name: 'Project One', workspace_id: 'workspace-1', workspace_name: 'Workspace One' }] })
    }, { preconnect: originalFetch.preconnect })
    const view = render(<Providers><OAuthConsentPage /></Providers>)
    await view.findByText('Remote client')
    expect(view.getByText('127.0.0.1').tagName).toBe('STRONG')
    await userEvent.click(view.getByRole('checkbox', { name: 'Client runs on a remote server' }))
    if (approved) {
      fireEvent.click(view.getByRole('combobox', { name: /Workspace/ }))
      await userEvent.click(await view.findByRole('option', { name: 'Workspace One' }))
      await userEvent.click(view.getByRole('checkbox', { name: 'Project One' }))
    }
    fireEvent.click(view.getByRole('button', { name: approved ? 'Allow read access' : 'Deny' }))
    const field = await view.findByRole('textbox', { name: 'Callback URL' }) as HTMLTextAreaElement
    expect(field.value).toBe(callback)
    expect(window.location.pathname).toBe('/oauth/consent')
    expect(body).toEqual({ csrf_token: 'nonce', approved, workspace_id: approved ? 'workspace-1' : null, project_ids: approved ? ['project-1'] : [], scopes: [] })
    expect(view.queryAllByRole('button', { name: 'Allow read access' }).length).toBe(0)
    const clipboard = navigator.clipboard
    const originalWrite = clipboard.writeText
    try {
      clipboard.writeText = async () => { throw new Error('Clipboard denied') }
      fireEvent.click(view.getByRole('button', { name: 'Copy callback URL' }))
      await view.findByText('Could not copy. Select the URL above and copy it manually.')
      expect(field.value).toBe(callback)
      let copied = ''
      clipboard.writeText = async (text) => { copied = text }
      fireEvent.click(view.getByRole('button', { name: 'Copy callback URL' }))
      await view.findByText('Callback URL copied.')
      expect(copied).toBe(callback)
    } finally { clipboard.writeText = originalWrite }
  })
}

test('select all projects and a permission preset send every project and scope', async () => {
  const { fireEvent } = await import('@testing-library/react')
  const userEvent = (await import('@testing-library/user-event')).default.setup()
  let body: unknown
  window.history.replaceState(null, '', `/oauth/consent?request=${'p'.repeat(43)}`)
  globalThis.fetch = Object.assign(async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = new URL(input instanceof Request ? input.url : String(input), window.location.origin).pathname
    if (path === '/api/v1/auth/me') return Response.json({ id: 'user-1', display_name: 'Ada' })
    if (init?.method === 'POST') {
      body = JSON.parse(init.body as string)
      return Response.json({ redirect_uri: 'http://localhost:9999/callback?code=x' })
    }
    return Response.json({ client_name: 'Client', redirect_to: '127.0.0.1', csrf_token: 'nonce', scope: 'tasks:read', projects: [
      { id: 'p1', name: 'One', workspace_id: 'w1', workspace_name: 'Workspace One' },
      { id: 'p2', name: 'Two', workspace_id: 'w1', workspace_name: 'Workspace One' },
      { id: 'p3', name: 'Other', workspace_id: 'w2', workspace_name: 'Workspace Two' },
    ] })
  }, { preconnect: originalFetch.preconnect })
  const view = render(<Providers><OAuthConsentPage /></Providers>)
  await view.findByText('Client')
  await userEvent.click(view.getByRole('checkbox', { name: 'Client runs on a remote server' }))
  fireEvent.click(view.getByRole('combobox', { name: /Workspace/ }))
  await userEvent.click(await view.findByRole('option', { name: 'Workspace One' }))
  await userEvent.click(view.getByRole('checkbox', { name: 'Select all' }))
  expect(view.getByRole('checkbox', { name: 'Two' }).getAttribute('aria-checked')).toBe('true')
  fireEvent.click(view.getByRole('combobox', { name: /Access/ }))
  await userEvent.click(await view.findByRole('option', { name: 'Full access' }))
  expect(view.getByRole('checkbox', { name: /^Read chat visible/ }).getAttribute('aria-checked')).toBe('true')
  fireEvent.click(view.getByRole('button', { name: 'Allow selected access' }))
  await view.findByRole('textbox', { name: 'Callback URL' })
  expect(body).toEqual({ csrf_token: 'nonce', approved: true, workspace_id: 'w1', project_ids: ['p1', 'p2'], scopes: ['tasks:write', 'docs:read', 'chat:read'] })
})
