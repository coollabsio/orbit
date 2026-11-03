import { afterEach, expect, spyOn, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { toast } from 'sonner'
import type { WorkspaceRecord } from '@orbit/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { registerConfirmationHandler } from '@/components/common/confirmAction'
import { AdminBackupsPage } from './AdminBackupsPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

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
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminBackupsPage /></QueryClientProvider>)
  await view.findByText('No backups yet.')
  fireEvent.click(view.getByRole('button', { name: 'Create backup now' }))
  const link = await view.findByRole('link', { name: /Download backup from/ })
  expect(link.getAttribute('href')).toBe('/api/v1/admin/backups/backup-1/download')
  expect(view.getByText('2.0 KB · 3 files · Orbit 0.1.0')).toBeTruthy()
})

test('deleting a backup asks first, then removes it from the list', async () => {
  let stored: object[] = [{ id: 'backup-1', kind: 'snapshot', created_at: Date.UTC(2026, 8, 28, 10), byte_size: 2048, file_count: 3, schema_version: 33, application_version: '0.1.0' }]
  const deleted: string[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'DELETE') {
      deleted.push(new URL(request.url).pathname)
      stored = []
      return new Response(null, { status: 204 })
    }
    return Response.json({ items: stored })
  }) as unknown as typeof fetch
  const unregister = registerConfirmationHandler(async () => true)
  try {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const view = render(<QueryClientProvider client={client}><AdminBackupsPage /></QueryClientProvider>)
    fireEvent.click(await view.findByRole('button', { name: /Delete backup from/ }))
    await view.findByText('No backups yet.')
    expect(deleted).toEqual(['/api/v1/admin/backups/backup-1'])
  } finally {
    unregister()
  }
})

test('a failed backup shows the reason from the server', async () => {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'POST') {
      return Response.json(
        { type: 'about:blank', title: 'Backup failed', status: 500, code: 'backup_upload_failed', detail: 'The backup was saved on this server, but the upload to S3 failed: object storage answered 403: AccessDenied', instance: '/api/v1/admin/backups', request_id: 'r' },
        { status: 500, headers: { 'content-type': 'application/problem+json' } },
      )
    }
    return Response.json({ items: [] })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(<QueryClientProvider client={client}><AdminBackupsPage /></QueryClientProvider>)
  await view.findByText('No backups yet.')
  fireEvent.click(view.getByRole('button', { name: 'Create backup now' }))
  expect((await view.findByRole('alert')).textContent).toBe('The backup was saved on this server, but the upload to S3 failed: object storage answered 403: AccessDenied')
})

type RestoreToast = { description?: string; action: { label: string; onClick: () => void } }

function browseApi(restores: object[]) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const path = new URL(request.url).pathname
    if (path.endsWith('/restore')) {
      restores.push({ path, body: await request.json() })
      return Response.json({ kind: 'page', id: 'page-copy', title: 'Handbook (copy)', path: '/docs/page-copy', missing_files: 2 }, { status: 201 })
    }
    if (path.endsWith('/items')) {
      return Response.json({ items: [{ id: 'page-1', kind: 'page', title: 'Handbook', parent_title: null, project_name: null, number: null, deleted: true, updated_at: Date.UTC(2026, 8, 27), child_count: 3 }] })
    }
    if (path.endsWith('/workspaces')) {
      return Response.json({ items: [{ id: 'ws-gone', name: 'Old', live: false }, { id: 'ws-other', name: 'Other', live: true }, { id: 'ws-1', name: 'Acme', live: true }] })
    }
    return Response.json({ items: [{ id: 'backup-1', kind: 'snapshot', location: 'local', created_at: Date.UTC(2026, 8, 28, 10), byte_size: 2048, file_count: 3, schema_version: 33, application_version: '0.1.0' }] })
  }) as unknown as typeof fetch
}

function renderBrowsable(currentWorkspaceId = 'ws-1') {
  const current = { id: currentWorkspaceId, name: 'Current' } as WorkspaceRecord
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  function Location() {
    const location = useLocation()
    return <output aria-label="location">{location.pathname + location.search}</output>
  }
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/admin/backups']}>
        <WorkspaceContext.Provider value={{ workspace: current, workspaces: [current], selectWorkspace: () => {} }}>
          <Routes>
            <Route path="/admin/backups" element={<AdminBackupsPage />} />
            <Route path="*" element={<Location />} />
          </Routes>
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

test('restoring a page with sub-pages copies them by default and the toast opens the copy in its workspace', async () => {
  const restores: object[] = []
  browseApi(restores)
  const success = spyOn(toast, 'success')
  try {
    const view = renderBrowsable()
    fireEvent.click(await view.findByRole('button', { name: /Browse backup from/ }))
    fireEvent.click(await view.findByRole('button', { name: 'Restore a copy of Handbook' }))
    fireEvent.click(await view.findByRole('button', { name: 'Restore copy' }))
    await waitFor(() => expect(success).toHaveBeenCalledTimes(1))
    expect(restores).toEqual([{ path: '/api/v1/admin/backups/backup-1/workspaces/ws-1/restore', body: { kind: 'page', id: 'page-1', include_children: true } }])
    const [message, options] = success.mock.calls[0] as unknown as [string, RestoreToast]
    expect(message).toBe('Restored as Handbook (copy)')
    expect(options.description).toBe('2 files were not in the backup.')
    act(() => options.action.onClick())
    expect((await view.findByLabelText('location')).textContent).toBe('/docs/page-copy?workspace=ws-1')
  } finally {
    success.mockRestore()
  }
})

test('unticking "Include sub-pages" restores the page alone', async () => {
  const restores: Array<{ body: object }> = []
  browseApi(restores)
  const success = spyOn(toast, 'success')
  try {
    const view = renderBrowsable()
    fireEvent.click(await view.findByRole('button', { name: /Browse backup from/ }))
    fireEvent.click(await view.findByRole('button', { name: 'Restore a copy of Handbook' }))
    await userEvent.click(await view.findByRole('checkbox', { name: 'Include sub-pages' }))
    fireEvent.click(view.getByRole('button', { name: 'Restore copy' }))
    await waitFor(() => expect(restores.length).toBe(1))
    expect(restores[0].body).toEqual({ kind: 'page', id: 'page-1', include_children: false })
  } finally {
    success.mockRestore()
  }
})

test('a copy restored into a workspace the admin is not a member of has no Open action', async () => {
  const restores: Array<{ path: string }> = []
  browseApi(restores)
  const success = spyOn(toast, 'success')
  try {
    const view = renderBrowsable('ws-mine')
    fireEvent.click(await view.findByRole('button', { name: /Browse backup from/ }))
    fireEvent.click(await view.findByRole('button', { name: 'Restore a copy of Handbook' }))
    fireEvent.click(await view.findByRole('button', { name: 'Restore copy' }))
    await waitFor(() => expect(success).toHaveBeenCalledTimes(1))
    expect(restores[0]?.path).toBe('/api/v1/admin/backups/backup-1/workspaces/ws-other/restore')
    const [, options] = success.mock.calls[0] as unknown as [string, Partial<RestoreToast>]
    expect(options.action).toBeUndefined()
  } finally {
    success.mockRestore()
  }
})
