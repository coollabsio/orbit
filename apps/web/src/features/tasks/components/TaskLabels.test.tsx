import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import type { LabelRecord } from '@/api/generated/types.gen'
import { TaskLabels } from './TaskLabels'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const labels: LabelRecord[] = [
  { id: 'label-used', workspace_id: 'workspace-1', name: 'Bug', color: '#ff0000', version: 0 },
  { id: 'label-unused', workspace_id: 'workspace-1', name: 'Needs review', color: '#123456', version: 0 },
]

function renderLabels(props: Partial<React.ComponentProps<typeof TaskLabels>> = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  )
  return render(
    <TaskLabels workspaceId="workspace-1" labelIds={['label-used']} labels={labels} onChange={mock(() => {})} {...props} />,
    { wrapper },
  )
}

test('task labels render names and colors and offer unused workspace labels', () => {
  const view = renderLabels()

  expect(view.getByText('Bug')).toBeTruthy()
  expect(view.queryByText('label-used')).toBeNull()
  fireEvent.click(view.getByRole('button', { name: 'Add label' }))
  expect(view.getByRole('button', { name: /Needs review/ })).toBeTruthy()
})

test('a new label can be created and attached when the workspace has none', async () => {
  const onChange = mock(() => {})
  const created = { id: 'label-new', workspace_id: 'workspace-1', name: 'Urgent', color: '#8b5cf6', version: 0 }
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    expect(request.method).toBe('POST')
    expect(new URL(request.url, 'http://orbit.test').pathname).toBe('/api/v1/workspaces/workspace-1/labels')
    expect(await request.json()).toEqual({ name: 'Urgent', color: '#8b5cf6' })
    return Response.json(created, { status: 201 })
  }) as typeof fetch
  const view = renderLabels({ labelIds: [], labels: [], onChange })
  fireEvent.click(view.getByRole('button', { name: 'Add label' }))
  await userEvent.type(view.getByLabelText('New label name'), 'Urgent')
  fireEvent.submit(view.getByLabelText('New label name').closest('form')!)
  await waitFor(() => expect(onChange).toHaveBeenCalledWith(['label-new']))
})
