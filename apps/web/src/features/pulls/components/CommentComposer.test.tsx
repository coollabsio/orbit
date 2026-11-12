import { afterEach, expect, test } from 'bun:test'
import { waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { CommentComposer } from './CommentComposer'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch; localStorage.clear() })

const ref = { owner: 'acme', repo: 'repo', number: 12 }
const target = { path: 'src/a.ts', side: 'RIGHT' as const, line: 3 }
const thread = { id: 'T_1', path: 'src/a.ts', side: 'RIGHT', line: 3, start_line: null, start_side: null, is_resolved: false, is_outdated: false, can_resolve: true, can_reply: true, comments: [] }
const problem = (status: number, detail: string) => new Response(JSON.stringify({ type: 'about:blank', title: 'x', status, code: 'github_refused', detail, instance: '/', request_id: 'r' }), { status, headers: { 'content-type': 'application/problem+json' } })

function serve(reply: () => Response, sent: unknown[], pending: { id: string; comment_count: number } | null = null) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'POST') { sent.push(await request.json()); return reply() }
    return Response.json({ threads: [], pending_review: pending })
  }) as typeof fetch
}

test('Add to review posts a pending comment on the lines', async () => {
  const sent: unknown[] = []
  serve(() => Response.json(thread), sent)
  let done = 0
  const view = renderWithWorkspace(<CommentComposer workspaceId="workspace-1" pullRef={ref} headSha="h1" target={target} onDone={() => { done += 1 }} />)
  await userEvent.type(view.getByRole('textbox', { name: 'Comment on line 3' }), 'Nit: rename')
  await userEvent.click(view.getByRole('button', { name: 'Add to review' }))
  await waitFor(() => expect(done).toBe(1))
  expect(sent[0]).toEqual({ path: 'src/a.ts', side: 'RIGHT', line: 3, start_side: null, start_line: null, body: 'Nit: rename', mode: 'pending', head_sha: 'h1' })
})

test('Comment now posts a single comment', async () => {
  const sent: unknown[] = []
  serve(() => Response.json(thread), sent)
  const view = renderWithWorkspace(<CommentComposer workspaceId="workspace-1" pullRef={ref} headSha="h1" target={target} onDone={() => {}} />)
  await userEvent.type(view.getByRole('textbox', { name: 'Comment on line 3' }), 'Now')
  await userEvent.click(view.getByRole('button', { name: 'Comment now' }))
  await waitFor(() => expect((sent[0] as { mode: string }).mode).toBe('single'))
})

test('a refused comment keeps the text and shows why', async () => {
  serve(() => problem(422, 'GitHub says: line must be part of the diff'), [])
  const view = renderWithWorkspace(<CommentComposer workspaceId="workspace-1" pullRef={ref} headSha="h1" target={target} onDone={() => {}} />)
  const box = view.getByRole('textbox', { name: 'Comment on line 3' }) as HTMLTextAreaElement
  await userEvent.type(box, 'Keep me')
  await userEvent.click(view.getByRole('button', { name: 'Add to review' }))
  expect(await view.findByText('GitHub says: line must be part of the diff')).toBeTruthy()
  expect(box.value).toBe('Keep me')
})

test('a range names both ends', () => {
  serve(() => Response.json(thread), [])
  const view = renderWithWorkspace(<CommentComposer workspaceId="workspace-1" pullRef={ref} headSha="h1" target={{ ...target, line: 9, start_line: 5, start_side: 'RIGHT' }} onDone={() => {}} />)
  expect(view.getByRole('textbox', { name: 'Comment on lines 5 to 9' })).toBeTruthy()
})

test('while a review is pending, comments can only go into it', async () => {
  serve(() => Response.json(thread), [], { id: 'R_1', comment_count: 2 })
  const view = renderWithWorkspace(<CommentComposer workspaceId="workspace-1" pullRef={ref} headSha="h1" target={target} onDone={() => {}} />)
  await view.findByRole('button', { name: 'Add to review' })
  await waitFor(() => expect(view.queryAllByRole('button', { name: 'Comment now' }).length).toBe(0))
})
