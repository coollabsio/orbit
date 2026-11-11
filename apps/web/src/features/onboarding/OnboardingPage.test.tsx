import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { OnboardingPage } from './OnboardingPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const WORKSPACE = { id: 'ws-1', name: 'Acme', role: 'owner', permissions: [], version: 0 }
const PROJECT = { id: 'p-1', name: 'General', key: 'GEN', color: '#5e6ad2', version: 0, auto_close_parent: true, auto_close_sub_issues: true, created_at: '2026-10-08T10:00:00.000Z', updated_at: '2026-10-08T10:00:00.000Z' }

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

function renderOnboarding(workspaces: object[] = []) {
  const requests: { method: string; path: string; body: unknown }[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const path = new URL(request.url).pathname
    const body = request.method === 'GET' ? undefined : await request.json()
    requests.push({ method: request.method, path, body })
    if (request.method === 'POST' && path === '/api/v1/workspaces') return Response.json(WORKSPACE, { status: 201 })
    if (path === '/api/v1/workspaces') return Response.json(workspaces)
    if (request.method === 'PATCH') return Response.json({ ...PROJECT, ...(body as object), version: 1 })
    return Response.json({ items: [PROJECT], next_cursor: null })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/onboarding']}>
        <Routes>
          <Route path="onboarding" element={<OnboardingPage />} />
          <Route path="tasks" element={<p>Tasks</p>} />
          <Route path="profile" element={<p>Account</p>} />
        </Routes>
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { view, requests }
}

test('the workspace step leads to naming the default project, whose tag follows the name', async () => {
  const { view, requests } = renderOnboarding()
  await userEvent.type(await view.findByLabelText('Workspace name'), 'Acme')
  await userEvent.click(view.getByRole('button', { name: 'Create workspace' }))
  expect(requests).toContainEqual({ method: 'POST', path: '/api/v1/workspaces', body: { name: 'Acme' } })

  await userEvent.type(await view.findByLabelText('Project name'), 'Website')
  expect((view.getByLabelText('Tag') as HTMLInputElement).value).toBe('WEB')
  await userEvent.click(view.getByRole('button', { name: 'Color #10b981' }))
  await userEvent.click(view.getByRole('button', { name: 'Create project' }))

  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/tasks?workspace=ws-1&project=p-1'))
  expect(requests).toContainEqual({
    method: 'PATCH',
    path: '/api/v1/workspaces/ws-1/projects/p-1',
    body: { name: 'Website', key: 'WEB', color: '#10b981', expected_version: 0 },
  })
})

test('someone who already has a workspace goes straight to the tasks', async () => {
  const { view } = renderOnboarding([WORKSPACE])
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/tasks'))
})

test('someone with no workspace can open their account settings instead, to delete the account (#39)', async () => {
  const { view } = renderOnboarding()
  await userEvent.click(await view.findByRole('link', { name: 'Account settings' }))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/profile'))
})
