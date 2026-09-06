import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, useLocation } from 'react-router'
import { WorkspaceProvider } from '../../features/workspaces/WorkspaceProvider'
import { WorkspaceSwitcher } from './WorkspaceSwitcher'

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

function setup() {
  window.localStorage.clear()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(['workspaces'], [
    { id: 'alpha', name: 'Alpha', role: 'owner', version: 1 },
    { id: 'beta', name: 'Beta', role: 'member', version: 1 },
  ])
  return render(<QueryClientProvider client={client}><MemoryRouter initialEntries={['/tasks?workspace=alpha']}>
    <WorkspaceProvider><WorkspaceSwitcher /><Location /></WorkspaceProvider>
  </MemoryRouter></QueryClientProvider>)
}

test('switching workspaces updates the current name, route and saved preference, then closes the menu', async () => {
  const view = setup()
  fireEvent.click(view.getByRole('button', { name: 'Workspace: Alpha' }))
  expect(view.getByRole('button', { name: 'Alpha' }).getAttribute('aria-current')).toBe('true')
  fireEvent.click(view.getByRole('button', { name: 'Beta' }))
  expect(await view.findByRole('button', { name: 'Workspace: Beta' })).toBeTruthy()
  expect(view.getByTestId('location').textContent).toBe('/tasks?workspace=beta')
  expect(window.localStorage.getItem('orbit:selected_workspace')).toBe('beta')
  expect(view.queryByRole('button', { name: 'Alpha' })).toBeNull()
})

test('the workspace menu can be dismissed without switching', () => {
  const view = setup()
  const trigger = view.getByRole('button', { name: 'Workspace: Alpha' })
  expect(trigger.getAttribute('aria-expanded')).toBe('false')
  fireEvent.click(trigger)
  expect(trigger.getAttribute('aria-expanded')).toBe('true')
  fireEvent.keyDown(document, { key: 'Escape' })
  expect(trigger.getAttribute('aria-expanded')).toBe('false')
  fireEvent.click(trigger)
  fireEvent.pointerDown(document.body)
  expect(trigger.getAttribute('aria-expanded')).toBe('false')
  expect(view.getByTestId('location').textContent).toBe('/tasks?workspace=alpha')
})

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

test('creates a workspace from the dropdown and selects it', async () => {
  let submittedName: string | undefined
  globalThis.fetch = (async (request: Request) => {
    submittedName = (await request.json()).name
    return Response.json({ id: 'gamma', name: 'Gamma', role: 'owner', version: 1 })
  }) as unknown as typeof fetch
  const view = setup()
  fireEvent.click(view.getByRole('button', { name: 'Workspace: Alpha' }))
  fireEvent.click(view.getByRole('button', { name: 'Create workspace' }))
  const input = view.getByLabelText('New workspace')
  expect((view.getByRole('button', { name: 'Create workspace' }) as HTMLButtonElement).disabled).toBe(true)
  await userEvent.type(input, '   ')
  expect((view.getByRole('button', { name: 'Create workspace' }) as HTMLButtonElement).disabled).toBe(true)
  await userEvent.clear(input)
  await userEvent.type(input, '  Gamma  ')
  fireEvent.submit(input.closest('form')!)
  expect(await view.findByRole('button', { name: 'Workspace: Gamma' })).toBeTruthy()
  expect(submittedName).toBe('Gamma')
  expect(view.getByTestId('location').textContent).toBe('/tasks?workspace=gamma')
  expect(view.queryByLabelText('New workspace')).toBeNull()
})

test('failed creation keeps the name for retry and cancel returns to workspace selection', async () => {
  globalThis.fetch = (async () => Response.json({ status: 500, title: 'Unavailable' }, { status: 500 })) as unknown as typeof fetch
  const view = setup()
  fireEvent.click(view.getByRole('button', { name: 'Workspace: Alpha' }))
  fireEvent.click(view.getByRole('button', { name: 'Create workspace' }))
  const input = view.getByLabelText('New workspace') as HTMLInputElement
  await userEvent.type(input, 'Gamma')
  fireEvent.submit(input.closest('form')!)
  expect(await view.findByRole('alert')).toBeTruthy()
  expect(input.value).toBe('Gamma')
  expect(view.getByTestId('location').textContent).toBe('/tasks?workspace=alpha')
  fireEvent.click(view.getByRole('button', { name: 'Cancel' }))
  expect(view.queryByLabelText('New workspace')).toBeNull()
  expect(view.getByRole('button', { name: 'Beta' })).toBeTruthy()
})

test('dismissing a pending creation prevents duplicate submission and a late workspace switch', async () => {
  let finish!: (response: Response) => void
  globalThis.fetch = (() => new Promise<Response>((resolve) => { finish = resolve })) as unknown as typeof fetch
  const view = setup()
  fireEvent.click(view.getByRole('button', { name: 'Workspace: Alpha' }))
  fireEvent.click(view.getByRole('button', { name: 'Create workspace' }))
  await userEvent.type(view.getByLabelText('New workspace'), 'Gamma')
  await userEvent.click(view.getByRole('button', { name: 'Create workspace' }))
  await view.findByRole('button', { name: 'Creating…' })
  fireEvent.keyDown(document, { key: 'Escape' })
  fireEvent.click(view.getByRole('button', { name: 'Workspace: Alpha' }))
  expect((view.getByRole('button', { name: 'Create workspace' }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(view.getByRole('button', { name: 'Beta' }))
  finish(Response.json({ id: 'gamma', name: 'Gamma', role: 'owner', version: 1 }))
  fireEvent.click(view.getByRole('button', { name: 'Workspace: Beta' }))
  await view.findByRole('button', { name: 'Gamma' })
  expect(view.getByTestId('location').textContent).toBe('/tasks?workspace=beta')
})
