import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { GeneralPage } from './GeneralPage'

function renderPage() {
  const client = new QueryClient()
  const workspace = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 7 }
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
  await userEvent.clear(view.getByLabelText('Name'))
  await userEvent.type(view.getByLabelText('Name'), ' Orbit ')
  expect(view.queryByRole('button', { name: 'Save Changes' })).toBeNull()
})
