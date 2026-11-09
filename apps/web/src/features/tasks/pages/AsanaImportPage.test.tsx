import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import type { AsanaImport, AsanaImportTree } from '@orbit/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { ConfirmationModalHost } from '@/components/common/ConfirmationModal'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { testWorkspace } from '@/test/workspace'
import { AsanaImportPage } from './AsanaImportPage'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const TREE: AsanaImportTree = {
  workspaces: [
    {
      gid: '100',
      name: 'Acme',
      projects: [
        { gid: '202', name: 'Old', archived: true, team_gid: null, team_name: null },
        { gid: '201', name: 'Ops', archived: false, team_gid: '9', team_name: 'Eng' },
        { gid: '200', name: 'Website Launch', archived: false, team_gid: '9', team_name: 'Eng' },
      ],
    },
  ],
  truncated: false,
}

const item = (patch: Partial<AsanaImport> = {}): AsanaImport => ({
  id: 'import-1', workspace_id: 'workspace-1', status: 'ready', asana_user_name: 'Ada', error: null,
  progress: { total: 0, done: 0, failed: 0 }, tree: null, report: null, project_ids: [],
  created_at: '2026-11-04T10:00:00Z', updated_at: '2026-11-04T10:00:00Z', ...patch,
})

const problem = (status: number, code: string) =>
  Response.json(
    { type: 'about:blank', title: 'Problem', status, code, detail: code, instance: '/imports', request_id: 'request-1' },
    { status, headers: { 'content-type': 'application/problem+json' } },
  )

type Call = { method: string; path: string; search: string; body: Record<string, unknown> | undefined }

function Location() {
  return <output data-testid="location">{useLocation().pathname}</output>
}

function setup(path: string, handler: (call: Call) => Response | undefined) {
  window.localStorage.clear()
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    const text = await request.text()
    const call: Call = {
      method: request.method,
      path: url.pathname.replace('/api/v1/workspaces/workspace-1', ''),
      search: url.search,
      body: text ? JSON.parse(text) : undefined,
    }
    calls.push(call)
    const response = handler(call)
    if (response) return response
    if (call.method === 'GET' && call.path === '/imports/asana') return Response.json({ items: [] })
    if (call.method === 'GET' && call.path === '/projects') return Response.json({ items: [], next_cursor: null })
    return problem(404, 'not_found')
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  client.setQueryData(queryKeys.workspaces, [testWorkspace('member', { id: 'workspace-1' })])
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[`${path}?workspace=workspace-1`]}>
        <ConfirmationModalHost />
        <WorkspaceProvider>
          <Routes>
            <Route path="tasks-import/:importId?" element={<AsanaImportPage />} />
          </Routes>
        </WorkspaceProvider>
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { view, calls }
}

test('connect: the token is a password field, links to Asana, and a valid token opens the scan', async () => {
  const { view, calls } = setup('/tasks-import', (call) => {
    if (call.method === 'POST' && call.path === '/imports/asana') {
      return call.body?.token === '2/good' ? Response.json(item({ status: 'scanning' }), { status: 201 }) : problem(422, 'asana_token_invalid')
    }
    if (call.path === '/imports/asana/import-1') return Response.json(item({ status: 'scanning' }))
  })
  const input = (await view.findByLabelText('Asana personal access token')) as HTMLInputElement
  expect(input.type).toBe('password')
  expect(view.getByText('https://app.asana.com/0/my-apps').getAttribute('href')).toBe('https://app.asana.com/0/my-apps')

  await userEvent.type(input, '2/bad')
  fireEvent.click(view.getByRole('button', { name: 'Find projects' }))
  expect((await view.findByRole('alert')).textContent).toContain('Asana rejected this token')

  await userEvent.type(input, '{Backspace}'.repeat(3) + 'good')
  fireEvent.click(view.getByRole('button', { name: 'Find projects' }))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/tasks-import/import-1'))
  expect(await view.findByText('Finding your Asana projects…')).toBeTruthy()
  expect(calls.filter((call) => call.method === 'POST').map((call) => call.body)).toEqual([{ token: '2/bad' }, { token: '2/good' }])
})

test('choose: projects are grouped by team, archived ones start unselected, and start sends the selection', async () => {
  const { view, calls } = setup('/tasks-import/import-1', (call) => {
    if (call.method === 'GET' && call.path === '/imports/asana/import-1') {
      return Response.json(item({ tree: call.search === '?include=tree' ? TREE : null }))
    }
    if (call.method === 'POST' && call.path === '/imports/asana/import-1/start') return Response.json(item({ status: 'queued' }))
  })
  expect(await view.findByRole('button', { name: 'Import 2 projects' })).toBeTruthy()
  expect(view.getByRole('checkbox', { name: 'Select Old' }).getAttribute('aria-checked')).toBe('false')
  expect(view.getByRole('checkbox', { name: 'Select team Eng' }).getAttribute('aria-checked')).toBe('true')
  expect(view.getByRole('checkbox', { name: 'Select all projects' }).getAttribute('aria-checked')).toBe('mixed')

  fireEvent.click(view.getByRole('button', { name: 'Ops' }))
  expect(await view.findByRole('button', { name: 'Import 1 project' })).toBeTruthy()
  expect(view.getByRole('checkbox', { name: 'Select team Eng' }).getAttribute('aria-checked')).toBe('mixed')

  fireEvent.click(view.getByRole('button', { name: 'Select all' }))
  fireEvent.click(await view.findByRole('button', { name: 'Import 3 projects' }))
  await waitFor(() => expect(calls.some((call) => call.path === '/imports/asana/import-1/start')).toBe(true))
  const start = calls.find((call) => call.path === '/imports/asana/import-1/start')
  const gids = (start?.body?.project_gids ?? []) as string[]
  expect([...gids].sort()).toEqual(['200', '201', '202'])
})

test('result: shows counts, notes, imported projects and failures', async () => {
  const done = item({
    status: 'completed',
    progress: { total: 5, done: 4, failed: 1 },
    project_ids: ['project-1'],
    report: {
      comments_imported: 7, files_imported: 2, missing_files: 1, links_added: 1, dependencies_linked: 3,
      unmatched_assignees: 0, skipped_subtasks: 0,
      failures: [{ asana_gid: '400', kind: 'task', title: 'Broken task', error: 'Orbit refused the task.' }],
    },
  })
  const { view } = setup('/tasks-import/import-1', (call) => {
    if (call.path === '/imports/asana/import-1') return Response.json(done)
    if (call.path === '/projects') {
      return Response.json({
        items: [{ id: 'project-1', workspace_id: 'workspace-1', name: 'Website Launch', key: 'WL', color: '#5e6ad2', auto_close_parent: true,
          auto_close_sub_issues: true, version: 0, deleted_at: null, created_at: done.created_at, updated_at: done.created_at }],
        next_cursor: null,
      })
    }
  })
  expect(await view.findByText('Import complete')).toBeTruthy()
  expect((await view.findByRole('link', { name: 'Website Launch' })).getAttribute('href')).toBe('/tasks?project=project-1')
  expect(view.getByText('3 dependencies became “blocked by” relations.')).toBeTruthy()
  expect(view.getByText('1 Asana files could not be downloaded.')).toBeTruthy()
  expect(view.getByText('Broken task')).toBeTruthy()
  expect(view.getByText('Orbit refused the task.')).toBeTruthy()
})
