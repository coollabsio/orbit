import { afterEach, beforeEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { queryKeys } from '@/api/queryKeys'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { GeneralPage } from './GeneralPage'
import { testWorkspace } from '@/test/workspace'

const originalFetch = globalThis.fetch
beforeEach(() => { globalThis.fetch = (async () => Response.json({ items: [] })) as unknown as typeof fetch })
afterEach(() => { globalThis.fetch = originalFetch })

function renderPage({ installationAdmin = true }: { installationAdmin?: boolean } = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.currentUser, { id: 'user-1', email: 'owner@orbit.test', display_name: 'Owner', installation_admin: installationAdmin })
  const workspace = testWorkspace('owner', { version: 7 })
  return render(
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
        <GeneralPage />
      </WorkspaceContext.Provider>
    </QueryClientProvider>,
  )
}

test('workspace edits use the shared save popup and Reset restores the saved name', async () => {
  const view = renderPage()
  await view.findByText('No backups yet.')
  expect(view.queryByRole('button', { name: 'Save workspace' })).toBeNull()
  expect(view.queryByRole('button', { name: 'Save Changes' })).toBeNull()
  const input = view.getByLabelText('Name') as HTMLInputElement
  await userEvent.clear(input)
  await userEvent.type(input, 'Renamed workspace')
  expect(view.getByRole('button', { name: 'Save Changes' })).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Reset' }))
  expect(input.value).toBe('Orbit')
  expect(view.queryByRole('button', { name: 'Save Changes' })).toBeNull()
})

test('unchanged trimmed names do not show the save popup', async () => {
  const view = renderPage()
  await view.findByText('No backups yet.')
  await userEvent.clear(view.getByLabelText('Name'))
  await userEvent.type(view.getByLabelText('Name'), ' Orbit ')
  expect(view.queryByRole('button', { name: 'Save Changes' })).toBeNull()
})

test('creating a backup refreshes the list with a download link', async () => {
  const stored: object[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'POST') {
      stored.push({ id: 'backup-1', kind: 'snapshot', created_at: Date.UTC(2026, 8, 28, 10), byte_size: 2048, file_count: 3, schema_version: 33, application_version: '0.1.0' })
      return Response.json({ id: 'backup-1' }, { status: 201 })
    }
    return Response.json({ items: stored })
  }) as unknown as typeof fetch
  const view = renderPage()
  await view.findByText('No backups yet.')
  fireEvent.click(view.getByRole('button', { name: 'Create backup now' }))
  const link = await view.findByRole('link', { name: /Download backup from/ })
  expect(link.getAttribute('href')).toBe('/api/v1/admin/backups/backup-1/download')
  expect(view.getByText('2.0 KB · 3 files · Orbit 0.1.0')).toBeTruthy()
})

test('a user who is not an installation administrator gets no Backup card and no backup request', async () => {
  let requests = 0
  globalThis.fetch = (async () => {
    requests += 1
    return Response.json({ items: [] })
  }) as unknown as typeof fetch
  const view = renderPage({ installationAdmin: false })
  await view.findByLabelText('Version')
  await new Promise((resolve) => setTimeout(resolve, 20))
  expect(view.queryAllByText('Backup')).toHaveLength(0)
  expect(requests).toBe(0)
})
