import { afterEach, expect, test } from 'bun:test'
import { waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { ReviewPanel } from './ReviewPanel'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const ref = { owner: 'acme', repo: 'repo', number: 12 }
const viewer = { can_comment: true, can_review: true, can_merge: true, is_author: false }

function serve(pending: { id: string; comment_count: number } | null, sent: unknown[] = []) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'POST') { sent.push(await request.json()); return new Response(null, { status: 204 }) }
    return Response.json({ threads: [], pending_review: pending })
  }) as typeof fetch
  return sent
}

test('approving sends the summary with the event', async () => {
  const sent = serve(null)
  const view = renderWithWorkspace(<ReviewPanel workspaceId="workspace-1" pullRef={ref} viewer={viewer} />)
  await userEvent.click(view.getByRole('button', { name: 'Review' }))
  await userEvent.type(await view.findByRole('textbox', { name: 'Review summary' }), 'Ship it')
  await userEvent.click(view.getByRole('radio', { name: 'Approve' }))
  await userEvent.click(view.getByRole('button', { name: 'Submit review' }))
  await waitFor(() => expect(sent).toEqual([{ event: 'APPROVE', body: 'Ship it' }]))
})

test('the author of the pull request can only comment', async () => {
  serve(null)
  const view = renderWithWorkspace(<ReviewPanel workspaceId="workspace-1" pullRef={ref} viewer={{ ...viewer, is_author: true }} />)
  await userEvent.click(view.getByRole('button', { name: 'Review' }))
  await view.findByRole('textbox', { name: 'Review summary' })
  expect(view.queryAllByRole('radio', { name: 'Approve' }).length).toBe(0)
  expect(view.queryAllByRole('radio', { name: 'Request changes' }).length).toBe(0)
})

test('an empty comment review cannot be sent, and says why', async () => {
  serve(null)
  const view = renderWithWorkspace(<ReviewPanel workspaceId="workspace-1" pullRef={ref} viewer={viewer} />)
  await userEvent.click(view.getByRole('button', { name: 'Review' }))
  await view.findByRole('textbox', { name: 'Review summary' })
  expect((view.getByRole('button', { name: 'Submit review' }) as HTMLButtonElement).disabled).toBe(true)
  expect(view.getByText('Write a summary or add comments first.')).toBeTruthy()
})

test('the button shows how many comments are pending', async () => {
  serve({ id: 'R_1', comment_count: 3 })
  const view = renderWithWorkspace(<ReviewPanel workspaceId="workspace-1" pullRef={ref} viewer={viewer} />)
  expect(await view.findByRole('button', { name: 'Review, 3 pending comments' })).toBeTruthy()
})
