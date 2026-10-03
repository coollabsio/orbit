import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { queryKeys } from '@/api/queryKeys'
import type { User } from '@/features/workspaces/models'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { MembersPage } from './MembersPage'
import { testWorkspace } from '@/test/workspace'
import { NO_MEMBER_ABILITIES } from '@/features/workspaces/models'

const member = (id: string, role: User['role'], can: Partial<User['can']> = {}): User => ({
  id, membershipId: `membership-${id}`, name: id, handle: id, email: `${id}@orbit.test`, role,
  color: '#000', title: '', roleIds: [], version: 1, can: { ...NO_MEMBER_ABILITIES, ...can },
})

test('mounted member management excludes ordinary owner roles and exposes protected transfer', () => {
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } })
  client.setQueryData(queryKeys.currentUser, { id: 'owner', email: 'owner@orbit.test', display_name: 'Owner' })
  client.setQueryData(queryKeys.members('workspace-1'), [member('owner', 'Owner'), member('teammate', 'Member', { changeRole: true, remove: true, transferOwnership: true })])
  client.setQueryData(queryKeys.invitations('workspace-1'), { items: [{
    id: 'accepted', workspace_id: 'workspace-1', email: 'accepted@orbit.test', role: 'member',
    status: 'accepted', delivery: 'manual', created_at: '', expires_at: '2026-09-12T00:00:00Z',
  }], next_cursor: null })
  const workspace = testWorkspace('owner', { version: 7 })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider></QueryClientProvider>
  const view = render(<MembersPage />, { wrapper })

  const inviteRole = view.container.querySelector<HTMLButtonElement>('#invite-role')!
  fireEvent.click(inviteRole)
  expect(view.queryByRole('option', { name: 'Owner' })).toBeNull()
  expect(view.queryByText('accepted@orbit.test')).toBeNull()
  fireEvent.click(view.getByRole('button', { name: 'Manage' }))
  expect(view.getByRole('menuitem', { name: 'Transfer ownership' })).toBeTruthy()
})

test('page size options escape the card and changing size resets pagination', async () => {
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } })
  client.setQueryData(queryKeys.currentUser, { id: 'member-0' })
  client.setQueryData(queryKeys.members('pagination-workspace'), Array.from({ length: 30 }, (_, i) => member(`member-${i}`, 'Member')))
  const workspace = testWorkspace('member', { id: 'pagination-workspace' })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider></QueryClientProvider>
  const view = render(<MembersPage />, { wrapper })

  fireEvent.click(view.getByRole('button', { name: 'Next page' }))
  expect(view.getByText('11-20 of 30')).toBeTruthy()
  const trigger = view.getByRole('combobox', { name: 'Items per page' })
  // fireEvent opens the Base UI select trigger; the listbox content is portaled out of the card.
  fireEvent.click(trigger)
  const option = await view.findByRole('option', { name: '25' })
  expect(view.container.contains(option)).toBe(false)
  expect(view.getAllByRole('option').map((item) => item.textContent)).toEqual(['10', '25', '50', '100'])
  await userEvent.click(option)
  expect(trigger.getAttribute('aria-expanded')).toBe('false')
  // Base UI's Select.Icon injects a default '▼' text child into the chevron, so read the value slot.
  expect(trigger.querySelector('[data-slot="select-value"]')?.textContent).toBe('25')
  expect(view.getByText('1-25 of 30')).toBeTruthy()
  expect(view.container.querySelectorAll('[data-member-row]')).toHaveLength(25)

  fireEvent.click(trigger)
  await userEvent.keyboard('{Escape}')
  expect(trigger.getAttribute('aria-expanded')).toBe('false')
  fireEvent.click(trigger)
  await userEvent.click(document.body)
  expect(trigger.getAttribute('aria-expanded')).toBe('false')
})
