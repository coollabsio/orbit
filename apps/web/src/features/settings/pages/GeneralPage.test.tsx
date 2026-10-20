import { afterEach, beforeEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { queryKeys } from '@/api/queryKeys'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { GeneralPage } from './GeneralPage'
import { testWorkspace } from '@/test/workspace'

const originalFetch = globalThis.fetch
beforeEach(() => { globalThis.fetch = (async () => Response.json({ items: [] })) as unknown as typeof fetch })
afterEach(() => { globalThis.fetch = originalFetch })

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.currentUser, { id: 'user-1', email: 'owner@orbit.test', display_name: 'Owner', installation_admin: true })
  const workspace = testWorkspace('owner', { version: 7 })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
          <GeneralPage />
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

test('workspace edits use the shared save popup and Reset restores the saved name', async () => {
  const view = renderPage()
  await view.findByLabelText('Name')
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
  await view.findByLabelText('Name')
  await userEvent.clear(view.getByLabelText('Name'))
  await userEvent.type(view.getByLabelText('Name'), ' Orbit ')
  expect(view.queryByRole('button', { name: 'Save Changes' })).toBeNull()
})
