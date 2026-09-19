import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { queryKeys } from '../../api/queryKeys'
import type { User } from '../tasks/api/models'
import { WorkspaceContext } from '../workspaces/workspaceContext'
import { MembersPage } from './MembersPage'

const member = (id: string, role: User['role']): User => ({
  id, membershipId: `membership-${id}`, name: id, handle: id, email: `${id}@orbit.test`, role,
  color: '#000', online: false, title: '', roleIds: [], version: 1,
})

test('mounted member management excludes ordinary owner roles and exposes protected transfer', () => {
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } })
  client.setQueryData(queryKeys.currentUser, { id: 'owner', email: 'owner@orbit.test', display_name: 'Owner' })
  client.setQueryData(queryKeys.members('workspace-1'), [member('owner', 'Owner'), member('teammate', 'Member')])
  client.setQueryData(queryKeys.invitations('workspace-1'), { items: [{
    id: 'accepted', workspace_id: 'workspace-1', email: 'accepted@orbit.test', role: 'member',
    status: 'accepted', delivery: 'manual', created_at: '', expires_at: '2026-09-12T00:00:00Z',
  }], next_cursor: null })
  const workspace = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 7 }
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider></QueryClientProvider>
  const view = render(<MembersPage />, { wrapper })

  const inviteRole = view.container.querySelector<HTMLButtonElement>('#invite-role')!
  fireEvent.click(inviteRole)
  expect(view.queryByRole('option', { name: 'Owner' })).toBeNull()
  expect(view.queryByText('accepted@orbit.test')).toBeNull()
  fireEvent.click(view.getByRole('button', { name: 'Manage' }))
  expect(view.getByRole('button', { name: 'Transfer ownership' })).toBeTruthy()
})

test('page size options escape the card and changing size resets pagination', async () => {
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } })
  client.setQueryData(queryKeys.currentUser, { id: 'member-0' })
  client.setQueryData(queryKeys.members('pagination-workspace'), Array.from({ length: 30 }, (_, i) => member(`member-${i}`, 'Member')))
  const workspace = { id: 'pagination-workspace', name: 'Orbit', role: 'member', version: 1 }
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider></QueryClientProvider>
  const view = render(<MembersPage />, { wrapper })

  fireEvent.click(view.getByRole('button', { name: 'Next page' }))
  expect(view.getByText('11-20 of 30')).toBeTruthy()
  const trigger = view.getByRole('button', { name: 'Items per page' })
  // fireEvent opens the Base UI popover trigger; the listbox content is portaled out of the card.
  fireEvent.click(trigger)
  const option = await view.findByRole('option', { name: '25' })
  expect(view.container.contains(option)).toBe(false)
  expect(view.getAllByRole('option').map((item) => item.textContent)).toEqual(['10', '25', '50', '100'])
  await userEvent.click(option)
  expect(trigger.getAttribute('aria-expanded')).toBe('false')
  expect(trigger.textContent).toBe('25')
  expect(view.getByText('1-25 of 30')).toBeTruthy()
  expect(view.container.querySelectorAll('.data-table-row')).toHaveLength(25)

  fireEvent.click(trigger)
  await userEvent.keyboard('{Escape}')
  expect(trigger.getAttribute('aria-expanded')).toBe('false')
  fireEvent.click(trigger)
  await userEvent.click(document.body)
  expect(trigger.getAttribute('aria-expanded')).toBe('false')
})
