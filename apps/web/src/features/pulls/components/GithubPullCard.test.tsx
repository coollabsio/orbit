import { afterEach, expect, test } from 'bun:test'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { GithubPullCard } from './GithubPullCard'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const detail = {
  repository: 'acme/repo', number: 12, title: 'Fix the login', state: 'open', additions: 3, deletions: 1,
  author: { login: 'hubot', avatar_url: '' }, checks: { state: 'success', runs: [] },
}
const problem = (status: number, code: string) => new Response(JSON.stringify({ type: 'about:blank', title: 'x', status, code, detail: 'x', instance: '/', request_id: 'r' }), { status, headers: { 'content-type': 'application/problem+json' } })

test('a pull request the reader can see shows as a card that opens the viewer', async () => {
  globalThis.fetch = (async () => Response.json(detail)) as unknown as typeof fetch
  const view = renderWithWorkspace(<GithubPullCard workspaceId="workspace-1" pullRef={{ owner: 'acme', repo: 'repo', number: 12 }} />)
  const link = await view.findByRole('link', { name: /Fix the login/ })
  expect(link.getAttribute('href')).toBe('/tasks/pulls/acme/repo/12')
  expect(view.getByText('+3')).toBeTruthy()
})

test('a pull request the reader cannot see shows nothing (not even its title)', async () => {
  globalThis.fetch = (async () => problem(404, 'pull_not_found')) as unknown as typeof fetch
  const view = renderWithWorkspace(<GithubPullCard workspaceId="workspace-1" pullRef={{ owner: 'acme', repo: 'secret', number: 1 }} />)
  await new Promise((resolve) => setTimeout(resolve, 100))
  expect(view.container.textContent).toBe('')
})
