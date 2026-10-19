import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { registerConfirmationHandler } from '@/components/common/confirmAction'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { ProjectLabelsCard } from './ProjectLabelsCard'
import { testWorkspace } from '@/test/workspace'

const originalFetch = globalThis.fetch
const label = { id: 'label-1', workspace_id: 'workspace-1', project_id: 'project-1', name: 'Bug', color: '#ef4444', version: 3 }
const otherProjectLabel = { ...label, id: 'label-2', project_id: 'project-2', name: 'Other' }

afterEach(() => { globalThis.fetch = originalFetch })

function renderPage() {
  const requests: Request[] = []
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = new Request(input, init)
    requests.push(request)
    if (request.method === 'PATCH') return Response.json({ ...label, name: 'Defect', color: '#26b5ce', version: 4 })
    if (request.method === 'DELETE') return new Response(null, { status: 204 })
    if (request.method === 'POST') return Response.json({ ...label, id: 'label-3', name: 'Docs' }, { status: 201 })
    return Response.json({ items: [label, otherProjectLabel], next_cursor: null })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const workspace = testWorkspace('member')
  const view = render(<QueryClientProvider client={client}><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}><ProjectLabelsCard projectId="project-1" /></WorkspaceContext.Provider></QueryClientProvider>)
  return { view, requests }
}

test('editing a label sends the new name, color and expected version', async () => {
  const { view, requests } = renderPage()
  await userEvent.click(await view.findByRole('button', { name: 'Edit Bug' }))
  const input = view.getAllByLabelText('Label name').find((element) => (element as HTMLInputElement).value === 'Bug')!
  await userEvent.clear(input)
  await userEvent.type(input, 'Defect')
  await userEvent.click(view.getAllByRole('radio', { name: 'Teal' })[1]!)
  await userEvent.click(view.getByRole('button', { name: 'Save' }))
  await waitFor(() => expect(requests.some((request) => request.method === 'PATCH')).toBe(true))
  const request = requests.find((item) => item.method === 'PATCH')!
  expect(new URL(request.url).pathname).toBe('/api/v1/workspaces/workspace-1/projects/project-1/labels/label-1')
  expect(await request.json()).toEqual({ name: 'Defect', color: '#26b5ce', expected_version: 3 })
})

test('deleting a label asks first and sends the expected version', async () => {
  const unregister = registerConfirmationHandler(async () => true)
  try {
    const { view, requests } = renderPage()
    await userEvent.click(await view.findByRole('button', { name: 'Delete Bug' }))
    await waitFor(() => expect(requests.some((request) => request.method === 'DELETE')).toBe(true))
    const url = new URL(requests.find((item) => item.method === 'DELETE')!.url)
    expect(url.pathname).toBe('/api/v1/workspaces/workspace-1/projects/project-1/labels/label-1')
    expect(url.searchParams.get('expected_version')).toBe('3')
  } finally {
    unregister()
  }
})

test('the card lists and creates only the labels of its project', async () => {
  const { view, requests } = renderPage()
  await view.findByRole('button', { name: 'Edit Bug' })
  expect(view.queryAllByRole('button', { name: 'Edit Other' })).toHaveLength(0)
  await userEvent.type(view.getByLabelText('Label name'), 'Docs')
  await userEvent.click(view.getByRole('button', { name: 'Create label' }))
  await waitFor(() => expect(requests.some((request) => request.method === 'POST')).toBe(true))
  const request = requests.find((item) => item.method === 'POST')!
  expect(new URL(request.url).pathname).toBe('/api/v1/workspaces/workspace-1/projects/project-1/labels')
  expect(await request.json()).toEqual({ name: 'Docs', color: '#5e6ad2' })
})
