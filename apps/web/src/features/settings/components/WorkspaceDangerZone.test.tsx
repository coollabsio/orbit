import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, useLocation } from 'react-router'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import type { WorkspaceRecord } from '@/api/generated/types.gen'
import { DangerZonePage } from '@/features/settings/pages/DangerZonePage'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

function SelectedWorkspace() {
  return <output data-testid="selected-workspace">{useWorkspace().workspace.name}</output>
}

function setup(role = 'owner', last = false) {
  window.localStorage.clear()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  client.setQueryData(['workspaces'], [
    { id: 'alpha', name: 'Alpha', role, version: 7 },
    ...last ? [] : [{ id: 'beta', name: 'Beta', role: 'owner', version: 1 }],
  ])
  const view = render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/settings/danger-zone?workspace=alpha']}>
    <WorkspaceProvider><DangerZonePage /><SelectedWorkspace /></WorkspaceProvider><Location />
  </MemoryRouter></QueryClientProvider>)
  return { view, client }
}

test('only owners can delete and cancellation sends no request', async () => {
  let calls = 0
  globalThis.fetch = (async () => { calls++; return new Response(null, { status: 204 }) }) as unknown as typeof fetch
  const member = setup('member')
  expect(member.view.queryByRole('button', { name: 'Delete workspace' })).toBeNull()
  member.view.unmount()
  const { view } = setup()
  fireEvent.click(view.getByRole('button', { name: 'Delete workspace' }))
  const dialog = within(view.getByRole('dialog'))
  expect((dialog.getByRole('button', { name: 'Delete workspace' }) as HTMLButtonElement).disabled).toBe(true)
  await userEvent.type(dialog.getByLabelText('Confirm workspace name'), 'wrong')
  expect((dialog.getByRole('button', { name: 'Delete workspace' }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(dialog.getByRole('button', { name: 'Cancel' }))
  expect(view.queryByRole('dialog')).toBeNull()
  expect(calls).toBe(0)
})

test('confirmed deletion sends the workspace version, removes it and selects another workspace', async () => {
  let request: Request | undefined
  globalThis.fetch = (async (input: Request) => { request = input; return new Response(null, { status: 204 }) }) as unknown as typeof fetch
  const { view, client } = setup()
  fireEvent.click(view.getByRole('button', { name: 'Delete workspace' }))
  const dialog = within(view.getByRole('dialog'))
  await userEvent.type(dialog.getByLabelText('Confirm workspace name'), 'Alpha')
  await userEvent.click(dialog.getByRole('button', { name: 'Delete workspace' }))
  await view.findByText('Beta', { selector: 'output' })
  expect(request?.method).toBe('DELETE')
  expect(request?.url).toBe('http://localhost/api/v1/workspaces/alpha?expected_version=7')
  expect(client.getQueryData<WorkspaceRecord[]>(['workspaces'])).toEqual([{ id: 'beta', name: 'Beta', role: 'owner', version: 1 }])
  expect(view.getByTestId('location').textContent).toBe('/settings?workspace=beta')
  expect(view.queryByRole('dialog')).toBeNull()
})

test('failed deletion keeps the dialog open and permits retry', async () => {
  globalThis.fetch = (async () => Response.json({ status: 409, title: 'Conflict' }, { status: 409 })) as unknown as typeof fetch
  const { view } = setup()
  fireEvent.click(view.getByRole('button', { name: 'Delete workspace' }))
  const dialog = within(view.getByRole('dialog'))
  await userEvent.type(dialog.getByLabelText('Confirm workspace name'), 'Alpha')
  await userEvent.click(dialog.getByRole('button', { name: 'Delete workspace' }))
  expect(await dialog.findByRole('alert')).toBeTruthy()
  expect(view.getByTestId('location').textContent).toBe('/settings/danger-zone?workspace=alpha')
  expect((dialog.getByRole('button', { name: 'Delete workspace' }) as HTMLButtonElement).disabled).toBe(false)
})

test('deleting the last workspace warns about losing access and shows the no-access screen', async () => {
  globalThis.fetch = (async () => new Response(null, { status: 204 })) as unknown as typeof fetch
  const { view } = setup('owner', true)
  fireEvent.click(view.getByRole('button', { name: 'Delete workspace' }))
  const dialog = within(view.getByRole('dialog'))
  expect(dialog.getByText(/last workspace/i)).toBeTruthy()
  await userEvent.type(dialog.getByLabelText('Confirm workspace name'), 'Alpha')
  await userEvent.click(dialog.getByRole('button', { name: 'Delete workspace' }))
  expect(await view.findByRole('heading', { name: 'No workspace access' })).toBeTruthy()
})

test('pending deletion blocks duplicate requests and dialog dismissal', async () => {
  let finish!: (response: Response) => void
  let calls = 0
  globalThis.fetch = (() => { calls++; return new Promise<Response>((resolve) => { finish = resolve }) }) as unknown as typeof fetch
  const { view } = setup()
  fireEvent.click(view.getByRole('button', { name: 'Delete workspace' }))
  const dialog = within(view.getByRole('dialog'))
  const input = dialog.getByLabelText('Confirm workspace name')
  await userEvent.type(input, 'Alpha')
  await userEvent.click(dialog.getByRole('button', { name: 'Delete workspace' }))
  await dialog.findByRole('button', { name: 'Deleting…' })
  fireEvent.submit(input.closest('form')!)
  fireEvent.keyDown(document, { key: 'Escape' })
  fireEvent.click(dialog.getByRole('button', { name: 'Close' }))
  expect(view.getByRole('dialog')).toBeTruthy()
  expect(calls).toBe(1)
  finish(new Response(null, { status: 204 }))
  await view.findByText('Beta', { selector: 'output' })
})
