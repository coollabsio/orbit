import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import type { LinearImport, LinearImportTree } from '@orbit/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { ConfirmationModalHost } from '@/components/common/ConfirmationModal'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { testWorkspace } from '@/test/workspace'
import { LinearImportPage } from './LinearImportPage'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const TREE: LinearImportTree = {
  organization: 'Acme',
  teams: [
    { id: 'team-eng', key: 'ENG', name: 'Engineering', issue_count: 12, archived: false },
    { id: 'team-ops', key: 'OPS', name: 'Ops', issue_count: 1, archived: false },
    { id: 'team-old', key: 'OLD', name: 'Old', issue_count: 4, archived: true },
  ],
  truncated: false,
}

const item = (patch: Partial<LinearImport> = {}): LinearImport => ({
  id: 'import-1', workspace_id: 'workspace-1', status: 'ready', linear_user_name: 'Ada', error: null,
  progress: { total: 0, done: 0, failed: 0, expected: 0, read: 0, created: 0, stage: null }, waiting_until: null, tree: null, report: null, project_ids: [],
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
    if (call.method === 'GET' && call.path === '/imports/linear') return Response.json({ items: [] })
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
            <Route path="tasks-import/linear/:importId?" element={<LinearImportPage />} />
          </Routes>
        </WorkspaceProvider>
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { view, calls }
}

test('connect: the API key is a password field, links to Linear, and a valid key opens the scan', async () => {
  const { view, calls } = setup('/tasks-import/linear', (call) => {
    if (call.method === 'POST' && call.path === '/imports/linear') {
      return call.body?.token === 'lin_api_good' ? Response.json(item({ status: 'scanning' }), { status: 201 }) : problem(422, 'linear_token_invalid')
    }
    if (call.path === '/imports/linear/import-1') return Response.json(item({ status: 'scanning' }))
  })
  const input = (await view.findByLabelText('Linear personal API key')) as HTMLInputElement
  expect(input.type).toBe('password')
  expect(view.getByText('https://linear.app/settings/account/security').getAttribute('href')).toBe('https://linear.app/settings/account/security')

  await userEvent.type(input, 'lin_api_bad')
  fireEvent.click(view.getByRole('button', { name: 'Find teams' }))
  expect((await view.findByRole('alert')).textContent).toContain('Linear rejected this API key')

  await userEvent.type(input, '{Backspace}'.repeat(3) + 'good')
  fireEvent.click(view.getByRole('button', { name: 'Find teams' }))
  await waitFor(() => expect(view.getByTestId('location').textContent).toBe('/tasks-import/linear/import-1'))
  expect(await view.findByText('Finding your Linear teams…')).toBeTruthy()
  expect(calls.filter((call) => call.method === 'POST').map((call) => call.body)).toEqual([{ token: 'lin_api_bad' }, { token: 'lin_api_good' }])
})

test('choose: archived teams start unselected, and start sends the selection', async () => {
  const { view, calls } = setup('/tasks-import/linear/import-1', (call) => {
    if (call.method === 'GET' && call.path === '/imports/linear/import-1') {
      return Response.json(item({ tree: call.search === '?include=tree' ? TREE : null }))
    }
    if (call.method === 'POST' && call.path === '/imports/linear/import-1/start') return Response.json(item({ status: 'queued' }))
  })
  expect(await view.findByRole('button', { name: 'Import 2 teams' })).toBeTruthy()
  expect(view.getByText('Acme')).toBeTruthy()
  expect(view.getByText('12 issues')).toBeTruthy()
  expect(view.getByRole('checkbox', { name: 'Select Old' }).getAttribute('aria-checked')).toBe('false')
  expect(view.getByRole('checkbox', { name: 'Select all teams' }).getAttribute('aria-checked')).toBe('mixed')

  fireEvent.click(view.getByRole('button', { name: 'Ops · OPS' }))
  expect(await view.findByRole('button', { name: 'Import 1 team' })).toBeTruthy()

  fireEvent.click(view.getByRole('button', { name: 'Select all' }))
  fireEvent.click(await view.findByRole('button', { name: 'Import 3 teams' }))
  await waitFor(() => expect(calls.some((call) => call.path === '/imports/linear/import-1/start')).toBe(true))
  const start = calls.find((call) => call.path === '/imports/linear/import-1/start')
  const ids = (start?.body?.team_ids ?? []) as string[]
  expect([...ids].sort()).toEqual(['team-eng', 'team-old', 'team-ops'])
})

test('result: shows counts, notes, imported projects and failures', async () => {
  const done = item({
    status: 'completed',
    progress: { total: 5, done: 4, failed: 1, expected: 5, read: 5, created: 5, stage: null },
    project_ids: ['project-1'],
    report: {
      comments_imported: 7, files_imported: 2, missing_files: 1, links_added: 1, relations_linked: 3,
      relations_skipped: 2, unmatched_assignees: 0, detached_sub_issues: 1,
      failures: [{ linear_id: 'issue-9', kind: 'issue', title: 'ENG-9 Broken issue', error: 'Orbit refused the task.' }],
    },
  })
  const { view } = setup('/tasks-import/linear/import-1', (call) => {
    if (call.path === '/imports/linear/import-1') return Response.json(done)
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
  expect(view.getByText('3 blocking, related and duplicate relations were linked.')).toBeTruthy()
  expect(view.getByText('2 relations were skipped: Orbit keeps one relation per pair of tasks and one duplicate per task.')).toBeTruthy()
  expect(view.getByText('1 sub-issues are at the top level (parent in another team, or more than five levels deep); their description names the parent.')).toBeTruthy()
  expect(view.getByText('1 Linear files could not be downloaded.')).toBeTruthy()
  expect(view.getByText('ENG-9 Broken issue')).toBeTruthy()
  expect(view.getByText('Orbit refused the task.')).toBeTruthy()
})
