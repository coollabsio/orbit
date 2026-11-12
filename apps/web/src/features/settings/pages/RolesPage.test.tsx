import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { RolesResponse, WorkspaceRole } from '@orbit/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { testWorkspace } from '@/test/workspace'
import { RolesPage } from './RolesPage'

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
})

const roles: RolesResponse = {
  grantable_permissions: ['members.manage', 'audit.view', 'chat.manage'],
  items: [{ id: 'role-1', name: 'Moderator', permissions: ['chat.manage'], member_count: 2, version: 1, created_at: '', updated_at: '' }],
}

function renderPage(role: WorkspaceRole) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const workspace = testWorkspace(role)
  return render(<QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}><RolesPage /></WorkspaceContext.Provider></QueryClientProvider>)
}

test('ordinary members cannot manage roles and load nothing', () => {
  globalThis.fetch = mock(() => Promise.reject(new Error('must not fetch'))) as unknown as typeof fetch
  const view = renderPage('member')
  expect(view.getByText('Only workspace owners and administrators can manage roles.')).toBeTruthy()
  expect(globalThis.fetch).not.toHaveBeenCalled()
})

test('a new role is sent in the server order and a refusal shows the server detail', async () => {
  const posted: unknown[] = []
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    if (request.method === 'POST') {
      posted.push(await request.json())
      return Response.json(
        { type: 'about:blank', title: 'Conflict', status: 409, detail: 'A role named Auditor already exists.', code: 'role_name_taken', instance: '', request_id: 'r' },
        { status: 409, headers: { 'content-type': 'application/problem+json' } },
      )
    }
    return Response.json(roles)
  }) as unknown as typeof fetch
  const view = renderPage('admin')

  expect(await view.findByText('Moderator')).toBeTruthy()
  expect(view.getByText('2 members')).toBeTruthy()
  expect(view.getByText('Manage chat')).toBeTruthy()

  fireEvent.click(view.getByRole('button', { name: 'Add role' }))
  await userEvent.type(await view.findByLabelText(/Name/), '  Auditor ')
  await userEvent.click(view.getByRole('checkbox', { name: 'View the audit log' }))
  await userEvent.click(view.getByRole('checkbox', { name: 'Invite and manage members' }))
  fireEvent.click(view.getAllByRole('button', { name: 'Add role' }).at(-1)!)

  expect((await view.findByRole('alert')).textContent).toBe('A role named Auditor already exists.')
  expect(posted).toEqual([{ name: 'Auditor', permissions: ['members.manage', 'audit.view'] }])
})
