import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import type { Teamspace } from '@orbit/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { Project } from '@/features/tasks/api/models'
import { testWorkspace } from '@/test/workspace'
import { ProjectDocsCard } from './ProjectDocsCard'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const teamspace = (id: string, name: string, project_id: string | null, version = 1): Teamspace => ({
  id, workspace_id: 'workspace-1', name, icon: null, position: 0, version, project_id, is_default: false,
  created_at: '2026-11-12T00:00:00Z', updated_at: '2026-11-12T00:00:00Z',
})
const project = { id: 'project-1', name: 'Website' } as Project

function renderCard(teamspaces: Teamspace[]) {
  const requests: { method: string; path: string; body: unknown }[] = []
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    const path = new URL(request.url).pathname
    const body: unknown = request.method === 'GET' ? null : await request.json()
    requests.push({ method: request.method, path, body })
    if (request.method === 'POST') return Response.json(teamspace('new', 'Website', null, 0), { status: 201 })
    if (request.method === 'PATCH') {
      const id = path.split('/').pop()!
      const patch = body as { project_id: string | null }
      return Response.json({ ...teamspace(id, id, patch.project_id), version: 2 })
    }
    if (path.endsWith('/teamspaces')) return Response.json({ items: teamspaces })
    return Response.json({ items: [] })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const workspace = testWorkspace('member')
  const view = render(
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
          <ProjectDocsCard project={project} />
        </WorkspaceContext.Provider>
      </QueryClientProvider>
    </MemoryRouter>,
  )
  return { view, requests }
}

test('"Create docs teamspace" makes a teamspace named after the project and links it', async () => {
  const { view, requests } = renderCard([teamspace('general', 'General', null)])
  const button = await view.findByRole('button', { name: 'Create docs teamspace' })
  await waitFor(() => expect(button.hasAttribute('disabled')).toBe(false))
  await userEvent.click(button)
  await waitFor(() => expect(requests.some((request) => request.method === 'PATCH')).toBe(true))
  const writes = requests.filter((request) => request.method !== 'GET')
  expect(writes).toEqual([
    { method: 'POST', path: '/api/v1/workspaces/workspace-1/teamspaces', body: { name: 'Website' } },
    { method: 'PATCH', path: '/api/v1/workspaces/workspace-1/teamspaces/new', body: { expected_version: 0, project_id: 'project-1' } },
  ])
})

test('choosing another teamspace unlinks the current one first, and other projects\' teamspaces are not offered', async () => {
  const { view, requests } = renderCard([
    teamspace('docs', 'Docs', 'project-1', 4),
    teamspace('general', 'General', null, 7),
    teamspace('mobile', 'Mobile', 'project-2'),
  ])
  await view.findByRole('button', { name: 'Open docs' })
  await userEvent.click(view.getByRole('combobox', { name: 'Docs teamspace' }))
  expect(view.queryByRole('option', { name: 'Mobile' })).toBeNull()
  await userEvent.click(await view.findByRole('option', { name: 'General' }))
  await waitFor(() => expect(requests.filter((request) => request.method === 'PATCH')).toHaveLength(2))
  expect(requests.filter((request) => request.method === 'PATCH')).toEqual([
    { method: 'PATCH', path: '/api/v1/workspaces/workspace-1/teamspaces/docs', body: { expected_version: 4, project_id: null } },
    { method: 'PATCH', path: '/api/v1/workspaces/workspace-1/teamspaces/general', body: { expected_version: 7, project_id: 'project-1' } },
  ])
})
