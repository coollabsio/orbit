import { afterEach, expect, test } from 'bun:test'
import { waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Route, Routes } from 'react-router'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { PullListPage } from './PullListPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const item = { repository: 'acme/repo', number: 12, title: 'Fix the login', state: 'open', author: { login: 'hubot', avatar_url: '' }, updated_at: Date.now() - 3_600_000, comments: 2 }
const problem = (status: number, code: string) => new Response(JSON.stringify({ type: 'about:blank', title: 'x', status, code, detail: 'x', instance: '/', request_id: 'r' }), { status, headers: { 'content-type': 'application/problem+json' } })

function serve(list: () => Response, seen: string[] = []) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = new URL((input as Request).url)
    if (url.pathname.endsWith('/github/pulls')) { seen.push(url.search); return list() }
    if (url.pathname.endsWith('/github/me')) return Response.json({ status: 'disconnected', login: null })
    if (url.pathname.endsWith('/projects')) return Response.json({ items: [{ id: 'project-1', name: 'Coolify' }], next_cursor: null })
    return Response.json({})
  }) as typeof fetch
}

const page = () => renderWithWorkspace(<Routes><Route path="/tasks/projects/:projectId/pulls" element={<PullListPage />} /></Routes>, { path: '/tasks/projects/project-1/pulls' })

test('lists the open pull requests first and links each to the viewer', async () => {
  const seen: string[] = []
  serve(() => Response.json({ items: [item], next_cursor: null }), seen)
  const view = page()
  const link = await view.findByRole('link', { name: /Fix the login/ })
  expect(link.getAttribute('href')).toBe('/tasks/projects/project-1/pulls?pr=acme/repo/12')
  expect(new URLSearchParams(seen[0]).get('filter')).toBe('open')
  expect(view.getByText('#12')).toBeTruthy()
})

test('a filter tab asks GitHub for that filter', async () => {
  const seen: string[] = []
  serve(() => Response.json({ items: [item], next_cursor: null }), seen)
  const view = page()
  await view.findByRole('link', { name: /Fix the login/ })
  await userEvent.click(view.getByRole('tab', { name: 'Needs my review' }))
  await waitFor(() => expect(seen.map((search) => new URLSearchParams(search).get('filter'))).toContain('review'))
})

test('a member without GitHub sees the connect prompt', async () => {
  serve(() => problem(409, 'github_not_connected'))
  const view = page()
  expect(await view.findByText('Connect GitHub to see this pull request.')).toBeTruthy()
})

test('an empty filter says so', async () => {
  serve(() => Response.json({ items: [], next_cursor: null }))
  const view = page()
  expect(await view.findByText('No open pull requests')).toBeTruthy()
})
