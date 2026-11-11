import { afterEach, expect, test } from 'bun:test'
import { waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { registerConfirmationHandler } from '@/components/common/confirmAction'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { ThreadView } from './ThreadView'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch; localStorage.clear() })

const ref = { owner: 'acme', repo: 'repo', number: 12 }
const author = { login: 'octo', avatar_url: '' }
const thread = {
  id: 'T_1', path: 'src/a.ts', side: 'RIGHT', line: 3, start_line: null, start_side: null, is_resolved: false, is_outdated: false, can_resolve: true, can_reply: true,
  comments: [{ id: 'C_1', database_id: 7, author, body: 'Rename this', created_at: Date.now(), pending: true, can_edit: true }],
}

function serve(calls: Array<{ method: string; path: string; body: unknown }>) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const path = new URL(request.url).pathname
    calls.push({ method: request.method, path, body: await request.json().catch(() => null) })
    if (path.endsWith('/resolved')) return Response.json({ ...thread, is_resolved: request.method === 'PUT' })
    if (path.endsWith('/replies')) return Response.json(thread.comments[0])
    if (request.method === 'DELETE') return new Response(null, { status: 204 })
    return Response.json({ threads: [thread], pending_review: null })
  }) as typeof fetch
}

const base = '/api/v1/workspaces/workspace-1/github/pulls/acme/repo/12'

test('a reply is posted to the thread', async () => {
  const calls: Array<{ method: string; path: string; body: unknown }> = []
  serve(calls)
  const view = renderWithWorkspace(<ThreadView thread={thread} actions={{ workspaceId: 'workspace-1', pullRef: ref }} />)
  await userEvent.click(view.getByRole('button', { name: 'Reply' }))
  await userEvent.type(view.getByRole('textbox', { name: 'Reply to the thread' }), 'Done')
  await userEvent.click(view.getByRole('button', { name: 'Send reply' }))
  await waitFor(() => expect(calls).toContainEqual({ method: 'POST', path: `${base}/threads/T_1/replies`, body: { body: 'Done' } }))
})

test('resolve marks the thread resolved on GitHub', async () => {
  const calls: Array<{ method: string; path: string; body: unknown }> = []
  serve(calls)
  const view = renderWithWorkspace(<ThreadView thread={thread} actions={{ workspaceId: 'workspace-1', pullRef: ref }} />)
  await userEvent.click(view.getByRole('button', { name: 'Resolve' }))
  await waitFor(() => expect(calls.some((call) => call.method === 'PUT' && call.path === `${base}/threads/T_1/resolved`)).toBe(true))
})

test('an own comment can be deleted after a confirm', async () => {
  const calls: Array<{ method: string; path: string; body: unknown }> = []
  serve(calls)
  const unregister = registerConfirmationHandler(async () => true)
  const view = renderWithWorkspace(<ThreadView thread={thread} actions={{ workspaceId: 'workspace-1', pullRef: ref }} />)
  await userEvent.click(view.getByRole('button', { name: 'Delete comment' }))
  await waitFor(() => expect(calls.some((call) => call.method === 'DELETE' && call.path === `${base}/comments/C_1`)).toBe(true))
  unregister()
})

test('without actions the thread is read-only', () => {
  const view = renderWithWorkspace(<ThreadView thread={thread} />)
  expect(view.queryAllByRole('button').length).toBe(0)
})
