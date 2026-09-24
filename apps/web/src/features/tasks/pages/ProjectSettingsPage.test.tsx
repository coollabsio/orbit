import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { ProjectSettingsPage } from './ProjectSettingsPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const workspace = { id: 'workspace-1', name: 'Orbit', role: 'owner' as const, version: 1 }
const project = { id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'ORB', color: '#e0457b', created_at: '', updated_at: '', version: 1 }
const status = (id: string, name: string, category: string, position: number) => ({
  id, workspace_id: 'workspace-1', project_id: 'project-1', name, description: '', color: '#8b8f98', category, position, version: 1,
})

function renderSettings() {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const path = new URL((input as Request).url).pathname
    if (path.endsWith('/statuses')) return Response.json({ items: [status('todo', 'Todo', 'unstarted', 0), status('dup', 'Duplicate', 'duplicate', 1)], next_cursor: null })
    if (path.endsWith('/projects')) return Response.json({ items: [project], next_cursor: null })
    if (path.endsWith('/tasks')) return Response.json({ items: [], next_cursor: null })
    return Response.json({ type: 'about:blank', title: 'Not found', status: 404, detail: 'missing', code: 'not_found', instance: path, request_id: 'request-1' }, { status: 404, headers: { 'content-type': 'application/problem+json' } })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
        <MemoryRouter initialEntries={['/tasks/projects/project-1/settings']}>
          <Routes><Route path="/tasks/projects/:projectId/settings" element={<ProjectSettingsPage />} /></Routes>
        </MemoryRouter>
      </WorkspaceContext.Provider>
    </QueryClientProvider>,
  )
}

test('the Duplicate status is a locked system status: renamable, not deletable, no siblings', async () => {
  const view = renderSettings()
  expect(await view.findByRole('img', { name: 'System status' })).toBeTruthy()
  expect(view.queryByRole('button', { name: 'Add duplicate status' })).toBeNull()
  expect(view.getByRole('button', { name: 'Add unstarted status' })).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Duplicate actions' }))
  const remove = await view.findByRole('menuitem', { name: /Delete/ }, { timeout: 5000 })
  expect(remove.hasAttribute('data-disabled')).toBe(true)
  expect(view.getByRole('menuitem', { name: /Edit/ }).hasAttribute('data-disabled')).toBe(false)
}, 20000)

test('the last regular status still cannot be deleted even though Duplicate exists', async () => {
  const view = renderSettings()
  fireEvent.click(await view.findByRole('button', { name: 'Todo actions' }))
  const remove = await view.findByRole('menuitem', { name: /Delete/ }, { timeout: 5000 })
  expect(remove.hasAttribute('data-disabled')).toBe(true)
}, 20000)
