import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import { useContext } from 'react'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { testWorkspace } from '@/test/workspace'
import { WorkspaceProvider } from './WorkspaceProvider'
import { WorkspaceContext } from './workspaceContext'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
  window.localStorage.clear()
})

function Page() {
  const location = useLocation().pathname
  const workspace = useContext(WorkspaceContext)?.workspace.name ?? 'no workspace'
  return <output data-testid="page">{`${location} ${workspace}`}</output>
}

function renderAt(path: string, { operator, workspaces }: { operator: boolean; workspaces: object[] }) {
  const user = { id: 'user-1', email: 'a@orbit.test', display_name: 'A', installation_admin: operator, root: operator }
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = new URL((input as Request).url)
    return Response.json(url.pathname === '/api/v1/workspaces' ? workspaces : user)
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="onboarding" element={<Page />} />
          <Route path="*" element={<WorkspaceProvider><Page /></WorkspaceProvider>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

test('a customer with no workspace is sent to the onboarding', async () => {
  const view = renderAt('/admin', { operator: false, workspaces: [] })
  await waitFor(() => expect(view.getByTestId('page').textContent).toBe('/onboarding no workspace'))
})

test('an operator with no workspace lands in Admin, without a workspace', async () => {
  const view = renderAt('/tasks', { operator: true, workspaces: [] })
  await waitFor(() => expect(view.getByTestId('page').textContent).toBe('/admin no workspace'))
})

test('an operator with no workspace may open their account settings', async () => {
  const view = renderAt('/profile/security', { operator: true, workspaces: [] })
  await waitFor(() => expect(view.getByTestId('page').textContent).toBe('/profile/security no workspace'))
})

test('an operator with a workspace gets the workspace app', async () => {
  const workspace = testWorkspace('owner')
  const view = renderAt('/tasks', { operator: true, workspaces: [workspace] })
  await waitFor(() => expect(view.getByTestId('page').textContent).toBe(`/tasks ${workspace.name}`))
})
