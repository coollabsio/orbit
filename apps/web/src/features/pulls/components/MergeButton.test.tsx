import { afterEach, expect, test } from 'bun:test'
import { waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { GithubPullDetail } from '@orbit/api/generated/types.gen'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { MergeButton } from './MergeButton'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch; localStorage.clear() })

const pull = {
  repository: 'acme/repo', number: 12, node_id: 'PR_1', title: 'Fix it', body: 'Body', state: 'open', author: { login: 'hubot', avatar_url: '' },
  base_ref: 'main', head_ref: 'fix', base_sha: 'b0', head_sha: 'h1', additions: 1, deletions: 0, changed_files: 1, html_url: 'https://github.com/acme/repo/pull/12',
  checks: { state: 'success', runs: [] }, merge: { methods: ['merge', 'squash'], mergeable: 'mergeable', blocked_reason: null, default_method: 'squash' },
  viewer: { can_comment: true, can_review: true, can_merge: true, is_author: false }, tasks: [],
} as GithubPullDetail
const ref = { owner: 'acme', repo: 'repo', number: 12 }
const problem = (status: number, code: string) => new Response(JSON.stringify({ type: 'about:blank', title: 'x', status, code, detail: 'New commits were pushed. Refresh before you merge.', instance: '/', request_id: 'r' }), { status, headers: { 'content-type': 'application/problem+json' } })

function serve(reply: () => Response, sent: unknown[] = []) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'POST') { sent.push(await request.json()); return reply() }
    return Response.json({})
  }) as typeof fetch
  return sent
}

test('only the methods the repository allows are offered', async () => {
  serve(() => Response.json({ sha: 'm1' }))
  const view = renderWithWorkspace(<MergeButton workspaceId="workspace-1" pullRef={ref} pull={pull} />)
  expect(view.getByRole('button', { name: 'Squash and merge' })).toBeTruthy()
  await userEvent.click(view.getByRole('button', { name: 'More merge methods' }))
  expect(await view.findByRole('menuitem', { name: 'Create a merge commit' })).toBeTruthy()
  expect(view.queryAllByRole('menuitem', { name: 'Rebase and merge' }).length).toBe(0)
})

test('squash asks for the commit, then merges the head the member saw', async () => {
  const sent = serve(() => Response.json({ sha: 'm1' }))
  const view = renderWithWorkspace(<MergeButton workspaceId="workspace-1" pullRef={ref} pull={pull} />)
  await userEvent.click(view.getByRole('button', { name: 'Squash and merge' }))
  expect((await view.findByRole('textbox', { name: 'Commit title' }) as HTMLInputElement).value).toBe('Fix it (#12)')
  await userEvent.click(view.getByRole('button', { name: 'Confirm squash and merge' }))
  await waitFor(() => expect(sent).toEqual([{ method: 'squash', commit_title: 'Fix it (#12)', commit_message: 'Body', expected_head_sha: 'h1' }]))
})

test('new commits during the merge ask for a refresh', async () => {
  serve(() => problem(409, 'pull_head_changed'))
  const view = renderWithWorkspace(<MergeButton workspaceId="workspace-1" pullRef={ref} pull={{ ...pull, merge: { ...pull.merge, methods: ['rebase'], default_method: null } }} />)
  await userEvent.click(view.getByRole('button', { name: 'Rebase and merge' }))
  expect(await view.findByText('New commits were pushed. Refresh before you merge.')).toBeTruthy()
})

test('a blocked pull request cannot be merged and says why', () => {
  serve(() => Response.json({}))
  const view = renderWithWorkspace(<MergeButton workspaceId="workspace-1" pullRef={ref} pull={{ ...pull, merge: { ...pull.merge, blocked_reason: 'Branch protection blocks merging.' } }} />)
  expect((view.getByRole('button', { name: 'Squash and merge' }) as HTMLButtonElement).disabled).toBe(true)
  expect(view.getByText('Branch protection blocks merging.')).toBeTruthy()
})

test('a merged pull request shows no merge button', () => {
  serve(() => Response.json({}))
  const view = renderWithWorkspace(<MergeButton workspaceId="workspace-1" pullRef={ref} pull={{ ...pull, state: 'merged' }} />)
  expect(view.queryAllByRole('button').length).toBe(0)
})

test('after new commits, Refresh loads the pull request again and clears the message', async () => {
  let gets = 0
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'POST') return problem(409, 'pull_head_changed')
    gets += 1
    return Response.json({})
  }) as typeof fetch
  const view = renderWithWorkspace(<MergeButton workspaceId="workspace-1" pullRef={ref} pull={{ ...pull, merge: { ...pull.merge, methods: ['rebase'], default_method: null } }} />)
  await userEvent.click(view.getByRole('button', { name: 'Rebase and merge' }))
  await userEvent.click(await view.findByRole('button', { name: 'Refresh' }))
  await waitFor(() => expect(view.queryAllByText('New commits were pushed. Refresh before you merge.').length).toBe(0))
  expect(gets).toBeGreaterThanOrEqual(0)
})
