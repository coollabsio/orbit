import { afterEach, expect, test } from 'bun:test'
import { waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Route, Routes } from 'react-router'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { PullPage } from './PullPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const actor = { login: 'hubot', avatar_url: '' }
const detail = {
  repository: 'acme/repo', number: 12, node_id: 'PR_1', title: 'Fix the login', body: 'It works now.', state: 'open', author: actor,
  base_ref: 'main', head_ref: 'fix', base_sha: 'b0', head_sha: 'h1', additions: 3, deletions: 1, changed_files: 3,
  html_url: 'https://github.com/acme/repo/pull/12', checks: { state: 'success', runs: [] },
  merge: { methods: ['squash'], mergeable: 'mergeable', blocked_reason: null, default_method: 'squash' },
  viewer: { can_comment: true, can_review: true, can_merge: true, is_author: false }, tasks: [{ task_id: 't1', title: 'Login fails' }],
}
const files = [
  { path: 'src/login.ts', previous_path: null, status: 'modified', additions: 2, deletions: 1, patch: '@@ -1 +1,2 @@\n const a = 1\n+const b = 2', viewed: false },
  { path: 'src/util.ts', previous_path: null, status: 'added', additions: 1, deletions: 0, patch: '@@ -0,0 +1 @@\n+export {}', viewed: true },
  { path: 'yarn.lock', previous_path: null, status: 'modified', additions: 9000, deletions: 0, patch: null, viewed: false },
]
const outdated = { id: 'T1', path: 'src/login.ts', side: 'RIGHT', line: null, start_line: null, start_side: null, is_resolved: false, is_outdated: true, can_resolve: true, can_reply: true,
  comments: [{ id: 'C1', database_id: 5, author: actor, body: 'Old remark', created_at: Date.now(), pending: false, can_edit: false }] }
const problem = (status: number, code: string) => new Response(JSON.stringify({ type: 'about:blank', title: 'x', status, code, detail: 'x', instance: '/', request_id: 'r' }), { status, headers: { 'content-type': 'application/problem+json' } })

function serve({ pull = () => Response.json(detail), writes = [] as Array<{ method: string; path: string; body: unknown }> } = {}) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const path = new URL(request.url).pathname
    if (request.method !== 'GET') { writes.push({ method: request.method, path, body: await request.json().catch(() => null) }); return new Response(null, { status: 204 }) }
    if (path.endsWith('/github/me')) return Response.json({ status: 'disconnected', login: null })
    if (path.endsWith('/acme/repo/12')) return pull()
    if (path.endsWith('/files')) return Response.json(files)
    if (path.endsWith('/threads')) return Response.json({ threads: [outdated], pending_review: null })
    if (path.endsWith('/conversation')) return Response.json([])
    return Response.json({})
  }) as typeof fetch
  return writes
}

const page = (path = '/tasks/pulls/acme/repo/12') => renderWithWorkspace(<Routes><Route path="/tasks/pulls/:owner/:repo/:number" element={<PullPage />} /></Routes>, { path })

test('the header shows the pull request and its linked task', async () => {
  serve()
  const view = page()
  expect(await view.findByRole('heading', { name: 'Fix the login' })).toBeTruthy()
  expect(view.getByText('#12')).toBeTruthy()
  expect(view.getByText('Open')).toBeTruthy()
  expect(view.getByText('main')).toBeTruthy()
  expect(view.getByRole('link', { name: 'Login fails' }).getAttribute('href')).toBe('/tasks/t1')
  expect(view.getByRole('link', { name: 'Open on GitHub' }).getAttribute('href')).toBe('https://github.com/acme/repo/pull/12')
})

test('the file tree lists every changed file with its viewed tick', async () => {
  serve()
  const view = page()
  const tree = await view.findByRole('tree', { name: 'Changed files' })
  await waitFor(() => expect(tree.querySelectorAll('[role="treeitem"][data-file]').length).toBe(3))
  expect((view.getByRole('checkbox', { name: 'Viewed src/util.ts' }) as HTMLInputElement).getAttribute('aria-checked')).toBe('true')
})

test('ticking a file as viewed tells GitHub', async () => {
  const writes = serve()
  const view = page()
  await userEvent.click(await view.findByRole('checkbox', { name: 'Viewed src/login.ts' }))
  await waitFor(() => expect(writes).toContainEqual({ method: 'PUT', path: '/api/v1/workspaces/workspace-1/github/pulls/acme/repo/12/viewed', body: { path: 'src/login.ts' } }))
})

test('a file GitHub sends no diff for links to GitHub', async () => {
  serve()
  const view = page()
  expect(await view.findByText('Too large to show here.')).toBeTruthy()
})

test('outdated comments are listed in the Overview tab', async () => {
  serve()
  const view = page()
  await userEvent.click(await view.findByRole('tab', { name: 'Overview' }))
  expect(await view.findByText('Old remark')).toBeTruthy()
  expect(view.getByText('It works now.')).toBeTruthy()
})

test('a member without GitHub sees the connect prompt', async () => {
  serve({ pull: () => problem(409, 'github_not_connected') })
  const view = page()
  expect(await view.findByText('Connect GitHub to see this pull request.')).toBeTruthy()
})

test('a pull request GitHub hides says so', async () => {
  serve({ pull: () => problem(404, 'pull_not_found') })
  const view = page()
  expect(await view.findByText('Pull request not found')).toBeTruthy()
})
