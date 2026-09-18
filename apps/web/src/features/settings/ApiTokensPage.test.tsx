import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { WorkspaceContext } from '../workspaces/workspaceContext'
import { ApiTokensPage } from './ApiTokensPage'

const originalFetch = globalThis.fetch

afterEach(() => { globalThis.fetch = originalFetch })

function renderPage(role: 'owner' | 'admin' | 'member') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const workspace = { id: 'workspace-1', name: 'Orbit', role, version: 1 }
  return render(<QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}><ApiTokensPage /></WorkspaceContext.Provider></QueryClientProvider>)
}

test('members cannot manage API tokens', () => {
  globalThis.fetch = mock(() => Promise.reject(new Error('must not fetch'))) as unknown as typeof fetch
  const view = renderPage('member')
  expect(view.getByText('Only workspace owners and administrators can manage API tokens.')).toBeTruthy()
  expect(globalThis.fetch).not.toHaveBeenCalled()
})

test('admins can create a token and see its secret once', async () => {
  const requests: Request[] = []
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    requests.push(request)
    if (request.method === 'POST') return Response.json({ id: 'token-1', name: 'Discord bot', project_ids: ['project-1', 'project-2'], token_prefix: 'orb_12345678', scopes: ['read', 'write'], created_at: '2026-09-18T10:00:00Z', last_used_at: null, token: 'orb_123456789-secret' }, { status: 201 })
    if (request.url.includes('/projects')) return Response.json({ items: [{ id: 'project-1', workspace_id: 'workspace-1', name: 'Support', project_key: 'SUP', color: '#ffffff', version: 0 }, { id: 'project-2', workspace_id: 'workspace-1', name: 'Platform', project_key: 'PLAT', color: '#ffffff', version: 0 }], next_cursor: null })
    return Response.json([])
  }) as unknown as typeof fetch
  const view = renderPage('admin')
  await waitFor(() => expect(view.getByLabelText('Projects')).toBeTruthy())
  await userEvent.click(view.getByLabelText('Projects'))
  await userEvent.click(view.getByRole('option', { name: 'Support' }))
  await userEvent.click(view.getByRole('option', { name: 'Platform' }))
  expect(view.getByLabelText('Projects').textContent).toContain('2 projects selected')
  await userEvent.type(view.getByPlaceholderText('Discord bot'), 'Discord bot')
  await userEvent.click(view.getByRole('button', { name: 'Create token' }))
  await waitFor(() => expect(view.getByText('orb_123456789-secret')).toBeTruthy())
  const request = requests.find((item) => item.method === 'POST')
  expect(request).toBeTruthy()
  expect(await request!.json()).toEqual({ name: 'Discord bot', project_ids: ['project-1', 'project-2'], scopes: ['read', 'write'] })
  expect(view.getByText('Copy this token now. You cannot see it again.')).toBeTruthy()
})
