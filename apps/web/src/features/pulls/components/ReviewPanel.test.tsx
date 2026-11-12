import { afterEach, expect, test } from 'bun:test'
import { waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { ReviewPanel } from './ReviewPanel'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const ref = { owner: 'acme', repo: 'repo', number: 12 }
const viewer = { login: 'me', can_comment: true, can_review: true, can_merge: true, is_author: false }

function serve(pending: { id: string; comment_count: number } | null, sent: unknown[] = []) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'POST') { sent.push(await request.json()); return new Response(null, { status: 204 }) }
    return Response.json({ threads: [], pending_review: pending })
  }) as typeof fetch
  return sent
}

test('approving is one click, and sends the summary with it', async () => {
  const sent = serve(null)
  const view = renderWithWorkspace(<ReviewPanel workspaceId="workspace-1" pullRef={ref} viewer={viewer} />)
  await userEvent.click(view.getByRole('button', { name: 'Review' }))
  await userEvent.type(await view.findByRole('textbox', { name: 'Review summary' }), 'Ship it')
  await userEvent.click(view.getByRole('button', { name: 'Approve' }))
  await waitFor(() => expect(sent).toEqual([{ event: 'APPROVE', body: 'Ship it' }]))
})

test('an approval needs no summary; Mod+Enter sends a comment review', async () => {
  const sent = serve(null)
  const view = renderWithWorkspace(<ReviewPanel workspaceId="workspace-1" pullRef={ref} viewer={viewer} />)
  await userEvent.click(view.getByRole('button', { name: 'Review' }))
  const box = await view.findByRole('textbox', { name: 'Review summary' })
  expect((view.getByRole('button', { name: 'Approve' }) as HTMLButtonElement).disabled).toBe(false)
  await userEvent.type(box, 'Two notes{Control>}{Enter}{/Control}')
  await waitFor(() => expect(sent).toEqual([{ event: 'COMMENT', body: 'Two notes' }]))
})

test('the author of the pull request can only comment', async () => {
  serve(null)
  const view = renderWithWorkspace(<ReviewPanel workspaceId="workspace-1" pullRef={ref} viewer={{ ...viewer, is_author: true }} />)
  await userEvent.click(view.getByRole('button', { name: 'Review' }))
  await view.findByRole('textbox', { name: 'Review summary' })
  expect(view.queryAllByRole('button', { name: 'Approve' }).length).toBe(0)
  expect(view.queryAllByRole('button', { name: 'Request changes' }).length).toBe(0)
})

test('an empty comment or change request cannot be sent', async () => {
  serve(null)
  const view = renderWithWorkspace(<ReviewPanel workspaceId="workspace-1" pullRef={ref} viewer={viewer} />)
  await userEvent.click(view.getByRole('button', { name: 'Review' }))
  await view.findByRole('textbox', { name: 'Review summary' })
  expect((view.getByRole('button', { name: 'Comment' }) as HTMLButtonElement).disabled).toBe(true)
  expect((view.getByRole('button', { name: 'Request changes' }) as HTMLButtonElement).disabled).toBe(true)
})

test('the button shows how many comments are pending', async () => {
  serve({ id: 'R_1', comment_count: 3 })
  const view = renderWithWorkspace(<ReviewPanel workspaceId="workspace-1" pullRef={ref} viewer={viewer} />)
  expect(await view.findByRole('button', { name: 'Review, 3 pending comments' })).toBeTruthy()
})
