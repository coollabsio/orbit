import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { ApiTokensPage } from './ApiTokensPage'
import { testWorkspace } from '@/test/workspace'

const originalFetch = globalThis.fetch
const originalClipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard')

afterEach(() => {
  globalThis.fetch = originalFetch
  if (originalClipboard) Object.defineProperty(navigator, 'clipboard', originalClipboard)
  else Reflect.deleteProperty(navigator, 'clipboard')
})

function renderPage(role: 'owner' | 'admin' | 'member') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const workspace = testWorkspace(role)
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
    if (request.method === 'POST') return Response.json({ id: 'token-1', name: 'Discord bot', project_ids: ['project-1', 'project-2'], service_account_id: 'service-1', service_account_name: 'Discord bot', token_prefix: 'orb_12345678', scopes: ['tasks:read'], created_at: '2026-09-18T10:00:00Z', last_used_at: null, token: 'orb_123456789-secret' }, { status: 201 })
    if (request.url.includes('/projects')) return Response.json({ items: [{ id: 'project-1', workspace_id: 'workspace-1', name: 'Support', project_key: 'SUP', color: '#ffffff', version: 0 }, { id: 'project-2', workspace_id: 'workspace-1', name: 'Platform', project_key: 'PLAT', color: '#ffffff', version: 0 }], next_cursor: null })
    return Response.json([])
  }) as unknown as typeof fetch
  const view = renderPage('admin')
  await waitFor(() => expect(view.getByRole('checkbox', { name: 'Support' })).toBeTruthy())
  await userEvent.click(view.getByRole('checkbox', { name: 'Select all' }))
  expect(view.getByRole('checkbox', { name: 'Support' }).getAttribute('aria-checked')).toBe('true')
  expect(view.getByRole('checkbox', { name: 'Platform' }).getAttribute('aria-checked')).toBe('true')
  expect(view.getByLabelText('Access preset').textContent).toContain('Tasks read and write')
  expect(view.getByLabelText('Expiration').textContent).toContain('Never expires')
  await userEvent.click(view.getByLabelText('Expiration'))
  await userEvent.click(view.getByRole('option', { name: '30 days' }))
  await userEvent.type(view.getByPlaceholderText('Discord bot'), 'Discord bot')
  const serviceAccount = view.getByRole('checkbox', { name: /Use a service account/ })
  expect(serviceAccount.getAttribute('aria-checked')).toBe('false')
  await userEvent.click(within(view.getByRole('group', { name: 'Tasks' })).getByRole('button', { name: 'Read' }))
  expect(view.getByLabelText('Access preset').textContent).toContain('Tasks read-only')
  await userEvent.click(serviceAccount)
  expect(serviceAccount.getAttribute('aria-checked')).toBe('true')
  expect(within(view.getByRole('group', { name: 'Docs' })).getByRole('button', { name: 'Read' }).hasAttribute('disabled')).toBe(true)
  await userEvent.click(view.getByRole('button', { name: 'Create token' }))
  await waitFor(() => expect(view.getByText('orb_123456789-secret')).toBeTruthy())
  const request = requests.find((item) => item.method === 'POST')
  expect(request).toBeTruthy()
  expect(await request!.json()).toEqual({ name: 'Discord bot', project_ids: ['project-1', 'project-2'], scopes: ['tasks:read'], service_account: true, expires_in_days: 30 })
  expect(view.getByText('Copy this token now. You cannot see it again.')).toBeTruthy()

  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
    writeText: async () => { throw new Error('Permission denied') },
  } })
  fireEvent.click(view.getByRole('button', { name: 'Copy' }))
  await new Promise((resolve) => setTimeout(resolve, 0))
  expect(view.queryByRole('button', { name: 'Copied' })).toBeNull()

  let finishWrite!: () => void
  let clipboardText = ''
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
    writeText: (text: string) => new Promise<void>((resolve) => {
      finishWrite = () => { clipboardText = text; resolve() }
    }),
  } })
  fireEvent.click(view.getByRole('button', { name: 'Copy' }))
  expect(view.queryByRole('button', { name: 'Copied' })).toBeNull()
  finishWrite()
  await waitFor(() => expect(view.getByRole('button', { name: 'Copied' })).toBeTruthy())
  expect(clipboardText).toBe('orb_123456789-secret')
})

test('a token without task access needs no project', async () => {
  const requests: Request[] = []
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    requests.push(request)
    if (request.method === 'POST') return Response.json({ id: 'token-1', name: 'Docs bot', project_ids: [], service_account_id: null, service_account_name: null, token_prefix: 'orb_12345678', scopes: ['docs:read'], created_at: '2026-09-18T10:00:00Z', last_used_at: null, token: 'orb_123456789-secret' }, { status: 201 })
    if (request.url.includes('/projects')) return Response.json({ items: [{ id: 'project-1', workspace_id: 'workspace-1', name: 'Support', project_key: 'SUP', color: '#ffffff', version: 0 }], next_cursor: null })
    return Response.json([])
  }) as unknown as typeof fetch
  const view = renderPage('admin')
  await waitFor(() => expect(view.getByRole('checkbox', { name: 'Support' })).toBeTruthy())
  await userEvent.type(view.getByPlaceholderText('Discord bot'), 'Docs bot')
  await userEvent.click(within(view.getByRole('group', { name: 'Tasks' })).getByRole('button', { name: 'No access' }))
  await userEvent.click(within(view.getByRole('group', { name: 'Docs' })).getByRole('button', { name: 'Read' }))
  await userEvent.click(view.getByRole('button', { name: 'Create token' }))
  await waitFor(() => expect(view.getByText('orb_123456789-secret')).toBeTruthy())
  const request = requests.find((item) => item.method === 'POST')
  expect(await request!.json()).toEqual({ name: 'Docs bot', project_ids: [], scopes: ['docs:read'], service_account: false, expires_in_days: null })
})

test('an expired token is marked as expired', async () => {
  const token = (name: string, expires_at: string | null) => ({ id: name, name, project_ids: [], service_account_id: null, service_account_name: null, token_prefix: 'orb_12345678', scopes: ['docs:read'], created_at: '2026-09-18T10:00:00Z', last_used_at: null, expires_at })
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    if (request.url.includes('/projects')) return Response.json({ items: [], next_cursor: null })
    return Response.json([token('Old bot', '2020-01-01T00:00:00Z'), token('New bot', '2999-01-01T00:00:00Z')])
  }) as unknown as typeof fetch
  const view = renderPage('admin')
  const oldRow = (await view.findByText('Old bot')).closest('[data-slot="settings-row"]') as HTMLElement
  const newRow = view.getByText('New bot').closest('[data-slot="settings-row"]') as HTMLElement
  expect(within(oldRow).getAllByText('Expired').length).toBe(1)
  expect(within(oldRow).getAllByText(/ · expired /).length).toBe(1)
  expect(within(newRow).queryAllByText('Expired').length).toBe(0)
  expect(within(newRow).getAllByText(/ · expires /).length).toBe(1)
})
