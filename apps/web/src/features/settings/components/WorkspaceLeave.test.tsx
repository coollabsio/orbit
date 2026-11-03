import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import type { WorkspaceRecord, WorkspaceRole } from '@orbit/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { ConfirmationModalHost } from '@/components/common/ConfirmationModal'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { testWorkspace } from '@/test/workspace'
import { WorkspaceLeave } from './WorkspaceLeave'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const BETA = testWorkspace('owner', { id: 'beta', name: 'Beta' })

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

function setup(role: WorkspaceRole, { last = false, billing = false } = {}) {
  window.localStorage.clear()
  const requests: Request[] = []
  const me = { id: 'user-1', email: 'me@orbit.test', display_name: 'Me', role, created_at: '2026-10-01T00:00:00Z', version: 4, can_change_role: false, can_remove: role !== 'owner', can_transfer_ownership: false }
  globalThis.fetch = (async (input: Request) => {
    requests.push(input)
    if (input.method === 'DELETE') return new Response(null, { status: 204 })
    return Response.json({ items: [{ ...me, id: 'membership-1', user_id: 'user-1' }], next_cursor: null })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  client.setQueryData(queryKeys.currentUser, { id: 'user-1', email: 'me@orbit.test', display_name: 'Me', installation_admin: false, root: false })
  client.setQueryData(queryKeys.authOptions, { billing_enabled: billing })
  client.setQueryData(queryKeys.workspaces, [testWorkspace(role, { id: 'alpha', name: 'Alpha' }), ...last ? [] : [BETA]])
  const view = render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/settings?workspace=alpha']}>
    <ConfirmationModalHost />
    <Routes>
      <Route path="onboarding" element={null} />
      <Route path="*" element={<WorkspaceProvider><WorkspaceLeave /></WorkspaceProvider>} />
    </Routes>
    <Location />
  </MemoryRouter></QueryClientProvider>)
  return { view, client, requests }
}

test('a member leaves after confirming and lands in another workspace', async () => {
  const { view, client, requests } = setup('member')
  const leave = await view.findByRole('button', { name: 'Leave workspace' })
  await waitFor(() => expect((leave as HTMLButtonElement).disabled).toBe(false))
  fireEvent.click(leave)
  const dialog = within(await view.findByRole('dialog'))
  fireEvent.click(dialog.getByRole('button', { name: 'Leave workspace' }))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/tasks?workspace=beta'))
  const removal = requests.find((request) => request.method === 'DELETE')
  expect(removal?.url).toBe('http://localhost/api/v1/workspaces/alpha/members/membership-1?expected_version=4')
  expect(client.getQueryData<WorkspaceRecord[]>(queryKeys.workspaces)).toEqual([BETA])
})

test('a member who leaves the last workspace is sent to the onboarding', async () => {
  const { view } = setup('member', { last: true })
  const leave = await view.findByRole('button', { name: 'Leave workspace' })
  await waitFor(() => expect((leave as HTMLButtonElement).disabled).toBe(false))
  fireEvent.click(leave)
  fireEvent.click(within(await view.findByRole('dialog')).getByRole('button', { name: 'Leave workspace' }))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/onboarding'))
})

test('cancelling sends nothing; the owner is told to transfer ownership first', async () => {
  const member = setup('member')
  const leave = await member.view.findByRole('button', { name: 'Leave workspace' })
  await waitFor(() => expect((leave as HTMLButtonElement).disabled).toBe(false))
  fireEvent.click(leave)
  fireEvent.click(within(await member.view.findByRole('dialog')).getByRole('button', { name: 'Cancel' }))
  await new Promise((resolve) => setTimeout(resolve, 50))
  expect(member.requests.filter((request) => request.method === 'DELETE')).toHaveLength(0)
  member.view.unmount()

  const owner = setup('owner')
  expect(await owner.view.findByText(/first transfer ownership/)).toBeTruthy()
  expect(owner.view.queryAllByRole('button', { name: 'Leave workspace' })).toHaveLength(0)
})

test('on Orbit Cloud the owner is told that ownership transfer is not available', async () => {
  const owner = setup('owner', { billing: true })
  expect(await owner.view.findByText('The owner cannot leave. Ownership transfer is not available on Orbit Cloud yet.')).toBeTruthy()
  expect(owner.view.queryAllByText(/first transfer ownership/)).toHaveLength(0)
})
