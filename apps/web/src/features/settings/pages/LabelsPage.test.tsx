import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import userEvent from '@testing-library/user-event'
import { registerConfirmationHandler } from '@/components/common/confirmAction'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { LabelsPage } from './LabelsPage'
import { testWorkspace } from '@/test/workspace'

const originalFetch = globalThis.fetch
const label = { id: 'label-1', workspace_id: 'workspace-1', name: 'Bug', color: '#ef4444', group_id: null, group_name: null, version: 3 }
const group = { id: 'group-1', workspace_id: 'workspace-1', name: 'Type', color: '#8b5cf6', position: 0, version: 0 }
const feature = { ...label, id: 'label-2', name: 'Feature', group_id: 'group-1', group_name: 'Type' }

afterEach(() => { globalThis.fetch = originalFetch })

function renderPage(patch: () => Response = () => Response.json({ ...label, name: 'Defect', color: '#0ea5e9', version: 4 })) {
  const requests: Request[] = []
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    requests.push(request.clone())
    if (request.method === 'PATCH') return patch()
    if (new URL(request.url).pathname.endsWith('/label-groups')) return Response.json({ items: [group] })
    if (request.method === 'GET') return Response.json({ items: [label, feature], next_cursor: null })
    if (request.method === 'DELETE') return new Response(null, { status: 204 })
    return Response.json({ items: [label], next_cursor: null })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const workspace = testWorkspace('member')
  const view = render(<QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}><MemoryRouter><LabelsPage /></MemoryRouter></WorkspaceContext.Provider></QueryClientProvider>)
  return { view, requests }
}

test('editing a label sends the new name, color and expected version', async () => {
  const { view, requests } = renderPage()
  await userEvent.click(await view.findByRole('button', { name: 'Edit Bug' }))
  const input = view.getAllByLabelText('Label name').find((element) => (element as HTMLInputElement).value === 'Bug')!
  await userEvent.clear(input)
  await userEvent.type(input, 'Defect')
  await userEvent.click(view.getAllByRole('button', { name: 'Color #0ea5e9' })[2]!)
  await userEvent.click(view.getByRole('button', { name: 'Save' }))
  await waitFor(() => expect(requests.some((request) => request.method === 'PATCH')).toBe(true))
  const request = requests.find((item) => item.method === 'PATCH')!
  expect(new URL(request.url).pathname).toBe('/api/v1/workspaces/workspace-1/labels/label-1')
  expect(await request.json()).toEqual({ name: 'Defect', color: '#0ea5e9', expected_version: 3 })
})

test('deleting a label asks first and sends the expected version', async () => {
  const unregister = registerConfirmationHandler(async () => true)
  try {
    const { view, requests } = renderPage()
    await userEvent.click(await view.findByRole('button', { name: 'Delete Bug' }))
    await waitFor(() => expect(requests.some((request) => request.method === 'DELETE')).toBe(true))
    const url = new URL(requests.find((item) => item.method === 'DELETE')!.url)
    expect(url.pathname).toBe('/api/v1/workspaces/workspace-1/labels/label-1')
    expect(url.searchParams.get('expected_version')).toBe('3')
  } finally {
    unregister()
  }
})

test('dropping a label on a group sends the group, and a refusal shows the task count and no label is lost', async () => {
  const { view, requests } = renderPage(() => Response.json({
    type: 'about:blank', title: 'Label group conflict', status: 409, code: 'label_group_conflict', detail: 'conflict', instance: '/api', request_id: 'request-1', conflict: { count: 2 },
  }, { status: 409, headers: { 'content-type': 'application/problem+json' } }))
  const row = (await view.findByRole('button', { name: 'Edit Bug' })).closest('[data-slot="settings-row"]') as HTMLElement
  const target = view.getByRole('region', { name: 'Group Type' })
  const dataTransfer = { setData: () => {}, getData: () => '', effectAllowed: 'all', dropEffect: 'none' }
  fireEvent.dragStart(row, { dataTransfer })
  // the dragged row stays in the document
  expect(row.isConnected).toBe(true)
  expect(row.getAttribute('data-dragging')).toBe('true')
  fireEvent.dragOver(target, { dataTransfer })
  fireEvent.drop(target, { dataTransfer })
  await waitFor(() => expect(requests.some((request) => request.method === 'PATCH')).toBe(true))
  expect(await requests.find((item) => item.method === 'PATCH')!.json()).toEqual({ name: 'Bug', color: '#ef4444', expected_version: 3, group_id: 'group-1' })
  const alert = await view.findByRole('alert')
  expect(alert.textContent).toContain('2 tasks have it and a different Type label')
  expect(view.getByRole('link', { name: 'Show the 2 tasks' })).toBeTruthy()
})

test('Ungroup takes a label out of its group', async () => {
  const { view, requests } = renderPage(() => Response.json({ ...feature, group_id: null, group_name: null, version: 4 }))
  await userEvent.click(await view.findByRole('button', { name: 'Remove Feature from Type' }))
  await waitFor(() => expect(requests.some((request) => request.method === 'PATCH')).toBe(true))
  expect(await requests.find((item) => item.method === 'PATCH')!.json()).toEqual({ name: 'Feature', color: '#ef4444', expected_version: 3, group_id: null })
})
