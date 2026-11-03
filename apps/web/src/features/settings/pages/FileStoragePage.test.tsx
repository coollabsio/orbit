import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { testWorkspace } from '@/test/workspace'
import { FileStoragePage } from './FileStoragePage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const S3 = { endpoint: 'https://s3.example.com', region: 'us-east-1', bucket: 'team-files', prefix: '', access_key_id: 'team', path_style: false }

function renderPage(role: 'owner' | 'admin' | 'member') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const workspace = testWorkspace(role)
  return { workspace, view: render(<QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}><FileStoragePage /></WorkspaceContext.Provider></QueryClientProvider>) }
}

test('members cannot manage file storage', () => {
  globalThis.fetch = mock(() => Promise.reject(new Error('must not fetch'))) as unknown as typeof fetch
  const { view } = renderPage('member')
  expect(view.getByText('Only workspace owners and administrators can manage file storage.')).toBeTruthy()
  expect(globalThis.fetch).not.toHaveBeenCalled()
})

test('an admin saves the workspace bucket and sees the files move to it', async () => {
  const requests: { method: string; path: string; body: unknown }[] = []
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    const body = request.method === 'PUT' ? await request.json() : null
    requests.push({ method: request.method, path: new URL(request.url).pathname, body })
    return Response.json(request.method === 'PUT'
      ? { s3: S3, removing: false, checking: false, files_to_move: 3, move_error: null }
      : { s3: null, removing: false, checking: false, files_to_move: 0, move_error: null })
  }) as unknown as typeof fetch
  const { view, workspace } = renderPage('admin')

  await userEvent.type(await view.findByLabelText(/^Endpoint/), 'https://s3.example.com')
  await userEvent.type(view.getByLabelText(/^Bucket/), 'team-files')
  await userEvent.type(view.getByLabelText(/^Access key ID/), 'team')
  await userEvent.type(view.getByLabelText(/^Secret access key/), 'secret')
  expect(view.queryAllByRole('button', { name: 'Remove' })).toHaveLength(0)
  await userEvent.click(view.getByRole('button', { name: 'Save bucket' }))

  await waitFor(() => expect(view.getByRole('status').textContent).toBe('3 files left to move to your bucket.'))
  expect(requests.find((request) => request.method === 'PUT')).toEqual({
    method: 'PUT',
    path: `/api/v1/workspaces/${workspace.id}/storage/bucket`,
    body: { endpoint: 'https://s3.example.com', region: null, bucket: 'team-files', prefix: null, access_key_id: 'team', secret_access_key: 'secret', path_style: false },
  })
  expect(view.getAllByRole('button', { name: 'Remove' })).toHaveLength(1)
})

test('a removed bucket shows the files moving back and offers no second removal', async () => {
  globalThis.fetch = mock(async () => Response.json({ s3: S3, removing: true, checking: false, files_to_move: 1, move_error: 'access denied.' })) as unknown as typeof fetch
  const { view } = renderPage('owner')
  await waitFor(() => expect(view.getByRole('status').textContent).toBe("1 file left to move back to Orbit's storage."))
  expect(view.getByRole('alert').textContent).toBe('Moving files stopped: access denied. Orbit tries again every ten minutes.')
  expect(view.queryAllByRole('button', { name: 'Remove' })).toHaveLength(0)
})

test('right after a save the page says it is counting the files and keeps asking', async () => {
  let calls = 0
  globalThis.fetch = mock(async () => {
    calls += 1
    return Response.json(calls === 1
      ? { s3: S3, removing: false, checking: true, files_to_move: 0, move_error: null }
      : { s3: S3, removing: false, checking: false, files_to_move: 4, move_error: null })
  }) as unknown as typeof fetch
  const { view } = renderPage('owner')
  await waitFor(() => expect(view.getByRole('status').textContent).toBe('Checking which files to move…'))
  await waitFor(() => expect(view.getByRole('status').textContent).toBe('4 files left to move to your bucket.'), { timeout: 7000 })
}, 10000)
