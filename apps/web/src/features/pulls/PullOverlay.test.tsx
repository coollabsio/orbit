import { afterEach, expect, test } from 'bun:test'
import { act, waitFor } from '@testing-library/react'
import { QueryClient } from '@tanstack/react-query'
import { pullKeys } from '@/features/pulls/api'
import userEvent from '@testing-library/user-event'
import { Route, Routes, useLocation } from 'react-router'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { PullOverlay, PullRedirect } from './PullOverlay'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch; localStorage.clear() })

const actor = { login: 'hubot', avatar_url: '' }
const detail = {
  repository: 'acme/repo', number: 12, node_id: 'PR_1', title: 'Fix the login', body: 'It works now.', state: 'open', author: actor,
  base_ref: 'main', head_ref: 'fix', base_sha: 'b0', head_sha: 'h1', additions: 3, deletions: 1, changed_files: 3,
  html_url: 'https://github.com/acme/repo/pull/12', checks: { state: 'success', runs: [] },
  merge: { methods: ['squash'], mergeable: 'mergeable', blocked_reason: null, default_method: 'squash' },
  viewer: { login: 'me', can_comment: true, can_review: true, can_merge: true, is_author: false }, tasks: [{ task_id: 't1', title: 'Login fails' }],
}
const files = [
  { path: 'src/login.ts', previous_path: null, status: 'modified', additions: 2, deletions: 1, patch: '@@ -1 +1,2 @@\n const a = 1\n+const b = 2', viewed: false },
  { path: 'src/util.ts', previous_path: null, status: 'added', additions: 1, deletions: 0, patch: '@@ -0,0 +1 @@\n+export {}', viewed: true },
  { path: 'yarn.lock', previous_path: null, status: 'modified', additions: 9000, deletions: 0, patch: null, viewed: false },
]
const outdated = { id: 'T1', path: 'src/login.ts', side: 'RIGHT', line: null, start_line: null, start_side: null, is_resolved: false, is_outdated: true, can_resolve: true, can_reply: true,
  comments: [{ id: 'C1', database_id: 5, author: actor, body: 'Old remark', created_at: Date.now(), pending: false, can_edit: false }] }
const problem = (status: number, code: string) => new Response(JSON.stringify({ type: 'about:blank', title: 'x', status, code, detail: 'x', instance: '/', request_id: 'r' }), { status, headers: { 'content-type': 'application/problem+json' } })

const onLockfile = { ...outdated, id: 'T2', path: 'yarn.lock', line: 3, is_outdated: false, comments: [{ ...outdated.comments[0], id: 'C2', database_id: 6, body: 'Why this bump?' }] }

function serve({ pull = () => Response.json(detail), writes = [] as Array<{ method: string; path: string; body: unknown }> } = {}) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const path = new URL(request.url).pathname
    if (request.method !== 'GET') { writes.push({ method: request.method, path, body: await request.json().catch(() => null) }); return new Response(null, { status: 204 }) }
    if (path.endsWith('/github/me')) return Response.json({ status: 'disconnected', login: null })
    if (path.endsWith('/acme/repo/12')) return pull()
    if (path.endsWith('/files')) return Response.json(files)
    if (path.endsWith('/threads')) return Response.json({ threads: [outdated, onLockfile], pending_review: null })
    if (path.endsWith('/conversation')) return Response.json([{ kind: 'review', author: { login: 'mia', avatar_url: '' }, body: 'Looks right.', created_at: 1, review_state: 'approved' }])
    return Response.json({})
  }) as typeof fetch
  return writes
}

function Where() {
  const location = useLocation()
  return <output aria-label="location">{location.pathname}{location.search}</output>
}

// the modal hides the page under it from the accessibility tree, so the location is read from the DOM
const where = () => document.querySelector('output[aria-label="location"]')?.textContent

const page = (path = '/tasks?pr=acme/repo/12', client?: QueryClient) => renderWithWorkspace(<Routes>
  <Route path="/tasks/pulls/:owner/:repo/:number" element={<PullRedirect />} />
  <Route path="*" element={<><Where /><PullOverlay /></>} />
</Routes>, { path, client })

test('a pull request opens over the page: title, state, and the linked task in the activity', async () => {
  localStorage.setItem('orbit:pulls:activity', '1')
  serve()
  const view = page()
  expect(await view.findByRole('heading', { name: 'Fix the login' })).toBeTruthy()
  expect(view.getByText('#12')).toBeTruthy()
  expect(view.getByText('Open')).toBeTruthy()
  expect(view.getByRole('link', { name: 'Login fails' }).getAttribute('href')).toBe('/tasks/t1')
  expect(view.getByRole('link', { name: 'Open on GitHub' }).getAttribute('href')).toBe('https://github.com/acme/repo/pull/12')
})

test('the old page URL opens the overlay over Tasks, with the comment', async () => {
  serve()
  const view = page('/tasks/pulls/acme/repo/12?comment=5')
  expect(await view.findByRole('heading', { name: 'Fix the login' })).toBeTruthy()
  expect(where()).toBe('/tasks?pr=acme/repo/12&prComment=5')
})

test('Esc closes the overlay and leaves the page under it', async () => {
  serve()
  const view = page('/tasks?view=board&pr=acme/repo/12')
  await view.findByRole('heading', { name: 'Fix the login' })
  await userEvent.keyboard('{Escape}')
  await waitFor(() => expect(where()).toBe('/tasks?view=board'))
})

test('Esc in a text field leaves the field first; the overlay stays open', async () => {
  serve()
  const view = page()
  await userEvent.click(await view.findByRole('textbox', { name: 'Filter files' }))
  await userEvent.keyboard('{Escape}')
  expect(where()).toBe('/tasks?pr=acme/repo/12')
  expect(document.activeElement?.getAttribute('aria-label')).not.toBe('Filter files')
})

test('files to review are in the tree; lockfiles are set aside, folded, and open on click', async () => {
  serve()
  const view = page()
  const tree = await view.findByRole('tree', { name: 'Changed files' })
  await waitFor(() => expect(tree.querySelectorAll('[role="treeitem"][data-file]').length).toBe(2))
  expect((view.getByRole('checkbox', { name: 'Viewed src/util.ts' }) as HTMLInputElement).getAttribute('aria-checked')).toBe('true')
  expect(view.getByRole('separator', { name: 'Files set aside' })).toBeTruthy()
  await userEvent.click(view.getByRole('button', { name: /yarn\.lock/ }))
  expect(await view.findByText('Too large to show here.')).toBeTruthy()
})

test('the summary counts the files to review and shows the latest decision', async () => {
  serve()
  const view = page()
  const summary = await view.findByRole('region', { name: 'Summary' })
  await waitFor(() => expect(summary.textContent).toContain('1 of 2 viewed'))
  expect(summary.textContent).toContain('1 set aside')
  await waitFor(() => expect(summary.textContent).toContain('Approved by mia'))
  expect(view.getByRole('progressbar', { name: 'Files viewed' }).getAttribute('aria-valuenow')).toBe('50')
})

test('ticking a file as viewed tells GitHub', async () => {
  const writes = serve()
  const view = page()
  await userEvent.click(await view.findByRole('checkbox', { name: 'Viewed src/login.ts' }))
  await waitFor(() => expect(writes).toContainEqual({ method: 'PUT', path: '/api/v1/workspaces/workspace-1/github/pulls/acme/repo/12/viewed', body: { path: 'src/login.ts' } }))
})

test('outdated comments and comments on files the diff cannot show are in the activity', async () => {
  localStorage.setItem('orbit:pulls:activity', '1')
  serve()
  const view = page()
  expect(await view.findByText('Old remark')).toBeTruthy()
  expect(view.getByText('Why this bump?')).toBeTruthy()
  expect(view.getByRole('region', { name: 'Comments not in the diff' })).toBeTruthy()
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

test('new commits show a bar; the files load again only on Refresh', async () => {
  let head = 'h1'
  let fileLoads = 0
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const path = new URL((input as Request).url).pathname
    if (path.endsWith('/acme/repo/12')) return Response.json({ ...detail, head_sha: head })
    if (path.endsWith('/files')) { fileLoads += 1; return Response.json(files) }
    if (path.endsWith('/threads')) return Response.json({ threads: [], pending_review: null })
    return Response.json([])
  }) as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = page('/tasks?pr=acme/repo/12', client)
  await view.findByRole('tree', { name: 'Changed files' })
  expect(fileLoads).toBe(1)
  head = 'h2'
  await act(() => client.invalidateQueries({ queryKey: pullKeys.pull('workspace-1', { owner: 'acme', repo: 'repo', number: 12 }), exact: true }))
  expect(await view.findByText('New commits were pushed.')).toBeTruthy()
  expect(fileLoads).toBe(1)
  await userEvent.click(view.getByRole('button', { name: 'Refresh' }))
  await waitFor(() => expect(fileLoads).toBe(2))
  await waitFor(() => expect(view.queryAllByText('New commits were pushed.').length).toBe(0))
})
