import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
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
