import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import type { LabelRecord } from '@/api/generated/types.gen'
import { MemoryRouter } from 'react-router'
import { LabelPicker } from './TaskLabels'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const labels: LabelRecord[] = [
  { id: 'label-used', workspace_id: 'workspace-1', project_id: 'project-1', name: 'Bug', color: '#ff0000', version: 0 },
  { id: 'label-unused', workspace_id: 'workspace-1', project_id: 'project-1', name: 'Needs review', color: '#123456', version: 0 },
]

function renderLabels(props: Partial<React.ComponentProps<typeof LabelPicker>> = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <MemoryRouter><QueryClientProvider client={client}>{children}</QueryClientProvider></MemoryRouter>
  )
  return render(
    <LabelPicker workspaceId="workspace-1" projectId="project-1" labelIds={['label-used']} labels={labels} onChange={mock(() => {})} trigger={<button type="button">Labels</button>} {...props} />,
    { wrapper },
  )
}

test('the picker lists the project labels, checks the chosen ones and toggles on select', async () => {
  const onChange = mock(() => {})
  const view = renderLabels({ onChange, labels: [...labels, { ...labels[1]!, id: 'label-other', project_id: 'project-2', name: 'Elsewhere' }] })

  fireEvent.click(view.getByRole('button', { name: 'Labels' }))
  expect((await view.findByRole('option', { name: 'Bug' })).getAttribute('aria-checked')).toBe('true')
  expect(view.queryAllByRole('option', { name: 'Elsewhere' })).toHaveLength(0)
  await userEvent.click(view.getByRole('option', { name: 'Needs review' }))
  expect(onChange).toHaveBeenCalledWith(['label-used', 'label-unused'])
})

test('a name the search does not find becomes a create row; Enter creates and attaches it', async () => {
  const onChange = mock(() => {})
  const created = { id: 'label-new', workspace_id: 'workspace-1', project_id: 'project-1', name: 'Urgent', color: '#5e6ad2', version: 0 }
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    expect(request.method).toBe('POST')
    expect(new URL(request.url, 'http://orbit.test').pathname).toBe('/api/v1/workspaces/workspace-1/projects/project-1/labels')
    // the first palette colour the project does not use yet
    expect(await request.json()).toEqual({ name: 'Urgent', color: '#5e6ad2' })
    return Response.json(created, { status: 201 })
  }) as typeof fetch
  const view = renderLabels({ labelIds: [], labels: [], onChange })
  fireEvent.click(view.getByRole('button', { name: 'Labels' }))
  expect(await view.findByText('This project has no labels yet. Type a name to create one.')).toBeTruthy()
  const search = view.getByRole('combobox', { name: 'Change labels' })
  await userEvent.type(search, 'Urgent{Enter}')
  await waitFor(() => expect(onChange).toHaveBeenCalledWith(['label-new']))
  await waitFor(() => expect((search as HTMLInputElement).value).toBe(''))
})

test('an existing name (any case) offers no create row', async () => {
  const view = renderLabels()
  fireEvent.click(view.getByRole('button', { name: 'Labels' }))
  await userEvent.type(await view.findByRole('combobox', { name: 'Change labels' }), 'bug')
  expect(view.getByRole('option', { name: 'Bug' })).toBeTruthy()
  expect(view.queryAllByRole('option', { name: /Create/ })).toHaveLength(0)
})

test("the task's labels come first", async () => {
  const view = renderLabels({ labelIds: ['label-unused'] })
  fireEvent.click(view.getByRole('button', { name: 'Labels' }))
  await view.findByRole('option', { name: 'Needs review' })
  expect(view.getAllByRole('option').map((option) => option.textContent)).toEqual(['Needs review', 'Bug'])
})

test('a search that matches no label still shows the create row and "Edit labels"', async () => {
  const view = renderLabels({ editLink: true })
  fireEvent.click(view.getByRole('button', { name: 'Labels' }))
  await userEvent.type(await view.findByRole('combobox', { name: 'Change labels' }), 'Docs')
  expect(view.getByRole('option', { name: /Create/ }).textContent).toContain('Docs')
  expect(view.getByRole('option', { name: /Edit labels/ })).toBeTruthy()
  expect(view.queryAllByRole('option', { name: 'Bug' })).toHaveLength(0)
})

test('a colour picked under the create row is the new label colour, and Enter still creates', async () => {
  let body: unknown
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    body = await request.json()
    return Response.json({ id: 'label-new', workspace_id: 'workspace-1', project_id: 'project-1', name: 'Docs', color: '#4cb782', version: 0 }, { status: 201 })
  }) as typeof fetch
  const onChange = mock(() => {})
  const view = renderLabels({ labelIds: [], onChange })
  fireEvent.click(view.getByRole('button', { name: 'Labels' }))
  const search = await view.findByRole('combobox', { name: 'Change labels' })
  await userEvent.type(search, 'Docs')
  fireEvent.click(view.getByRole('radio', { name: 'Green' }))
  expect(view.getByRole('radio', { name: 'Green' }).getAttribute('aria-checked')).toBe('true')
  await userEvent.type(search, '{Enter}')
  await waitFor(() => expect(onChange).toHaveBeenCalledWith(['label-new']))
  expect(body).toEqual({ name: 'Docs', color: '#4cb782' })
})
