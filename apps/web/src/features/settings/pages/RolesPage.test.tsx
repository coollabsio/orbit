import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ProjectRecord, RolesResponse, WorkspaceRole } from '@orbit/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { testWorkspace } from '@/test/workspace'
import { RolesPage } from './RolesPage'

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
})

const roles: RolesResponse = {
  grantable_permissions: ['members.manage', 'audit.view', 'chat.manage', 'comments.moderate', 'tasks.purge'],
  project_permissions: ['comments.moderate', 'tasks.purge'],
  items: [{
    id: 'role-1', name: 'Moderator', permissions: ['chat.manage'], member_count: 2, version: 3, created_at: '', updated_at: '',
    project_grants: [{ project_id: 'project-a', permissions: ['tasks.purge'] }],
  }],
}

const project = (id: string, name: string) => ({ id, name }) as ProjectRecord
const projects = { items: [project('project-a', 'Apollo'), project('project-b', 'Borealis')], next_cursor: null }

/** Answers the role and project lists; `write` answers POST and PATCH requests. */
function serve(write: (request: Request) => Promise<Response>) {
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    if (request.method !== 'GET') return write(request)
    return Response.json(new URL(request.url).pathname.endsWith('/projects') ? projects : roles)
  }) as unknown as typeof fetch
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
  serve(async (request) => {
    posted.push(await request.json())
    return Response.json(
      { type: 'about:blank', title: 'Conflict', status: 409, detail: 'A role named Auditor already exists.', code: 'role_name_taken', instance: '', request_id: 'r' },
      { status: 409, headers: { 'content-type': 'application/problem+json' } },
    )
  })
  const view = renderPage('admin')

  expect(await view.findByText('Moderator')).toBeTruthy()
  expect(view.getByText('2 members')).toBeTruthy()
  expect(view.getByText('Manage chat')).toBeTruthy()
  expect((await view.findByText('Apollo: Permanently delete tasks')).dataset.projectGrant).toBe('true')

  fireEvent.click(view.getByRole('button', { name: 'Add role' }))
  await userEvent.type(await view.findByLabelText(/Name/), '  Auditor ')
  await userEvent.click(view.getByRole('checkbox', { name: 'View the audit log' }))
  await userEvent.click(view.getByRole('checkbox', { name: 'Invite and manage members' }))
  // the new row takes the first project; an unticked row is not sent
  await userEvent.click(view.getByRole('button', { name: 'Add project' }))
  await userEvent.click(within(view.getByRole('group', { name: 'Apollo' })).getByRole('checkbox', { name: 'Moderate comments' }))
  await userEvent.click(view.getByRole('button', { name: 'Add project' }))
  expect(within(view.getByRole('group', { name: 'Borealis' })).getAllByRole('checkbox')).toHaveLength(2)
  // both projects are used, so there is none left to add
  expect((view.getByRole('button', { name: 'Add project' }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(view.getAllByRole('button', { name: 'Add role' }).at(-1)!)

  expect((await view.findByRole('alert')).textContent).toBe('A role named Auditor already exists.')
  expect(posted).toEqual([{ name: 'Auditor', permissions: ['members.manage', 'audit.view'], project_grants: [{ project_id: 'project-a', permissions: ['comments.moderate'] }] }])
})

test('editing a role prefills its project grants and sends the full list again', async () => {
  const patched: unknown[] = []
  serve(async (request) => {
    patched.push(await request.json())
    return Response.json(roles.items[0])
  })
  const view = renderPage('admin')

  fireEvent.click(await view.findByRole('button', { name: 'Edit' }))
  const apollo = await view.findByRole('group', { name: 'Apollo' })
  expect(within(apollo).getByRole('checkbox', { name: 'Permanently delete tasks' }).getAttribute('aria-checked')).toBe('true')
  await userEvent.click(within(apollo).getByRole('checkbox', { name: 'Moderate comments' }))
  // a project whose row is removed loses its grants: the list replaces the stored one
  await userEvent.click(view.getByRole('button', { name: 'Add project' }))
  const borealis = view.getByRole('group', { name: 'Borealis' })
  await userEvent.click(within(borealis).getByRole('checkbox', { name: 'Permanently delete tasks' }))
  await userEvent.click(within(borealis).getByRole('button', { name: 'Remove' }))
  fireEvent.click(view.getByRole('button', { name: 'Save' }))

  await waitFor(() => expect(patched).toHaveLength(1))
  expect(patched).toEqual([{
    expected_version: 3,
    name: 'Moderator',
    permissions: ['chat.manage'],
    project_grants: [{ project_id: 'project-a', permissions: ['comments.moderate', 'tasks.purge'] }],
  }])
})
