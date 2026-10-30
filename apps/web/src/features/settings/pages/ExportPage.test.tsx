import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { WorkspaceExport } from '@orbit/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { testWorkspace } from '@/test/workspace'
import { ExportPage } from './ExportPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const READY: WorkspaceExport = {
  id: 'export-1',
  status: 'ready',
  requested_by_name: 'Owner',
  size_bytes: 2048,
  created_at: '2026-10-30T10:00:00.000Z',
  completed_at: '2026-10-30T10:01:00.000Z',
  expires_at: '2026-11-06T10:01:00.000Z',
}

function renderPage(role: 'owner' | 'admin' | 'member') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const workspace = testWorkspace(role)
  return { workspace, view: render(<QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}><ExportPage /></WorkspaceContext.Provider></QueryClientProvider>) }
}

test('members cannot export the workspace', () => {
  globalThis.fetch = mock(() => Promise.reject(new Error('must not fetch'))) as unknown as typeof fetch
  const { view } = renderPage('member')
  expect(view.getByText('Only workspace owners and administrators can export the workspace.')).toBeTruthy()
  expect(globalThis.fetch).not.toHaveBeenCalled()
})

test('an admin starts an export, sees it being prepared and downloads it when ready', async () => {
  const requests: string[] = []
  let items: WorkspaceExport[] = []
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    requests.push(`${request.method} ${new URL(request.url).pathname}`)
    if (request.method === 'POST') {
      const queued = { ...READY, status: 'queued' as const, size_bytes: null, completed_at: null }
      items = [queued]
      return Response.json(queued, { status: 202 })
    }
    return Response.json({ items })
  }) as unknown as typeof fetch
  const { view, workspace } = renderPage('admin')

  await view.findByText('No exports yet.')
  await userEvent.click(view.getByRole('button', { name: 'Start export' }))
  await waitFor(() => expect(view.getByRole('status').textContent).toBe('Waiting to start…'))
  expect(view.getByRole('button', { name: 'Export in progress' }).hasAttribute('disabled')).toBe(true)
  expect(requests).toContain(`POST /api/v1/workspaces/${workspace.id}/exports`)

  items = [READY]
  const link = await view.findByRole('link', { name: /^Download export from/ }, { timeout: 5000 })
  expect(link.getAttribute('href')).toBe(`/api/v1/workspaces/${workspace.id}/exports/export-1/download`)
  expect(view.getByText(/^2.0 KB · Deleted on/)).toBeTruthy()
  expect(view.getByRole('button', { name: 'Start export' }).hasAttribute('disabled')).toBe(false)
}, 10000)
