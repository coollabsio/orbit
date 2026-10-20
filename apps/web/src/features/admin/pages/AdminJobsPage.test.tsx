import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { AdminJobsPage } from './AdminJobsPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const view = (kind: string, concurrency: number | null, workspace: number | null) => ({
  kind, concurrency, workspace_concurrency: workspace, default_concurrency: 2, default_workspace_concurrency: 1,
})

test('an empty field saves no limit, and reset puts the defaults back in the form', async () => {
  let stored = [view('notion.scan', 2, 1), view('notion.import', 2, 1)]
  const saved: unknown[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'PUT') {
      const body = await request.json() as { kinds: { kind: string; concurrency: number | null; workspace_concurrency: number | null }[] }
      saved.push(body)
      stored = body.kinds.map((kind) => view(kind.kind, kind.concurrency, kind.workspace_concurrency))
    }
    return Response.json({ kinds: stored })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const page = render(<QueryClientProvider client={client}><AdminJobsPage /></QueryClientProvider>)
  const user = userEvent.setup()

  await user.type(await page.findByLabelText('Notion imports on all servers'), '{Backspace}5')
  await user.type(page.getByLabelText('Notion imports per workspace'), '{Backspace}')
  fireEvent.click(page.getByRole('button', { name: 'Save limits' }))
  await waitFor(() => expect(saved).toHaveLength(1))
  expect(saved[0]).toEqual({ kinds: [
    { kind: 'notion.scan', concurrency: 2, workspace_concurrency: 1 },
    { kind: 'notion.import', concurrency: 5, workspace_concurrency: null },
  ] })

  await waitFor(() => expect((page.getByLabelText('Notion imports on all servers') as HTMLInputElement).value).toBe('5'))
  fireEvent.click(page.getByRole('button', { name: 'Reset to defaults' }))
  expect((page.getByLabelText('Notion imports on all servers') as HTMLInputElement).value).toBe('2')
  expect((page.getByLabelText('Notion imports per workspace') as HTMLInputElement).value).toBe('1')
})
