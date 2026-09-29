import { afterEach, expect, test } from 'bun:test'
import { fireEvent, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import type { SavedView } from '../api/views'
import { renderWithProviders, savedView, stubFetch } from '../testUtils'
import { DEFAULT_DISPLAY, defaultViewState, type ViewState } from '../viewState'
import { SaveViewDialog, type SaveViewMode } from './SaveViewDialog'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

test('creates a personal view from the current state with an icon and color', async () => {
  const requests = stubFetch((request) =>
    request.method === 'POST' ? Response.json(savedView({ id: 'view-new', name: 'Bugs' }), { status: 201 }) : Response.json([]),
  )
  const saved: SavedView[] = []
  const state: ViewState = { filter: { op: 'and', children: [{ field: 'label', operator: 'includes_any', value: ['label-bug'] }] }, display: DEFAULT_DISPLAY }
  const { view } = renderWithProviders(
    <SaveViewDialog open onOpenChange={() => {}} mode="create" workspaceId="alpha" state={state} onSaved={(next) => saved.push(next)} />,
  )
  await userEvent.click(view.getByRole('button', { name: 'Save view' }))
  expect(view.getByRole('alert').textContent).toBe('Enter a name.')

  await userEvent.type(view.getByRole('textbox', { name: 'Name' }), '  Bugs  ')
  const picker = view.getByRole('button', { name: 'Choose icon and color' })
  fireEvent.click(picker)
  await userEvent.click(await view.findByRole('button', { name: 'Bug icon' }))
  await userEvent.click(view.getByRole('button', { name: 'Red' }))
  fireEvent.click(picker)
  await userEvent.click(view.getByRole('button', { name: 'Save view' }))

  await waitFor(() => expect(saved.map((item) => item.id)).toEqual(['view-new']))
  expect(requests.find((request) => request.method === 'POST')?.body).toEqual({
    name: 'Bugs',
    description: '',
    icon: 'bug',
    color: '#eb5757',
    visibility: 'personal',
    state,
  })
})

test('non-owners edit a workspace view without visibility controls', async () => {
  const requests = stubFetch(() => Response.json(savedView({ id: 'view-1', name: 'Launch board', visibility: 'workspace', version: 4 })))
  const existing = savedView({ id: 'view-1', name: 'Launch', visibility: 'workspace', version: 3, owner: { user_id: 'user-2', display_name: 'Grace' } })
  const { view } = renderWithProviders(
    <SaveViewDialog open onOpenChange={() => {}} mode="edit" workspaceId="alpha" state={defaultViewState()} view={existing} onSaved={() => {}} />,
  )
  expect(view.getByRole('heading', { name: 'Edit view' })).toBeTruthy()
  expect(view.queryByRole('group', { name: 'Visibility' })).toBeNull()
  expect(view.getByText('Only the owner can change who sees this view.')).toBeTruthy()
  const name = view.getByRole('textbox', { name: 'Name' })
  await userEvent.clear(name)
  await userEvent.type(name, 'Launch board')
  await userEvent.click(view.getByRole('button', { name: 'Save changes' }))
  await waitFor(() => expect(requests.some((request) => request.method === 'PATCH')).toBe(true))
  expect(requests.find((request) => request.method === 'PATCH')?.body).toEqual({
    expected_version: 3,
    name: 'Launch board',
    description: '',
    icon: null,
    color: null,
  })
})

test('duplicates a view with its saved state under a copy name', async () => {
  const requests = stubFetch(() => Response.json(savedView({ id: 'view-2', name: 'Launch (copy)' }), { status: 201 }))
  const existing = savedView({
    id: 'view-1',
    name: 'Launch',
    visibility: 'workspace',
    state: { filter: { op: 'and', children: [{ field: 'priority', operator: 'is', value: ['high'] }] }, display: DEFAULT_DISPLAY },
  })
  const { view } = renderWithProviders(
    <SaveViewDialog open onOpenChange={() => {}} mode="duplicate" workspaceId="alpha" state={defaultViewState()} view={existing} onSaved={() => {}} />,
  )
  expect((view.getByRole('textbox', { name: 'Name' }) as HTMLInputElement).value).toBe('Launch (copy)')
  await userEvent.click(within(view.getByRole('group', { name: 'Visibility' })).getByRole('button', { name: 'Workspace' }))
  await userEvent.click(view.getByRole('button', { name: 'Duplicate view' }))
  await waitFor(() =>
    expect(requests.find((request) => request.method === 'POST')?.body).toMatchObject({ name: 'Launch (copy)', visibility: 'workspace', state: existing.state }),
  )
})

/** Like the Views page: the dialog state is cleared on close while the popup still plays its exit. */
function RowDialogs({ views }: { views: SavedView[] }) {
  const [dialog, setDialog] = useState<{ mode: SaveViewMode; view: SavedView } | null>(null)
  return (
    <>
      <button type="button" onClick={() => setDialog({ mode: 'duplicate', view: views[0]! })}>Duplicate A</button>
      <button type="button" onClick={() => setDialog({ mode: 'edit', view: views[1]! })}>Edit B</button>
      <SaveViewDialog
        open={dialog !== null}
        onOpenChange={(open) => {
          if (!open) setDialog(null)
        }}
        mode={dialog?.mode ?? 'edit'}
        workspaceId="alpha"
        state={defaultViewState()}
        view={dialog?.view}
        onSaved={() => {}}
      />
    </>
  )
}

test('the closing dialog keeps its title, and a quick reopen starts a fresh form', async () => {
  stubFetch(() => Response.json([]))
  const { view } = renderWithProviders(<RowDialogs views={[savedView({ id: 'view-a', name: 'Alpha' }), savedView({ id: 'view-b', name: 'Beta' })]} />)
  fireEvent.click(view.getByRole('button', { name: 'Duplicate A' }))
  await userEvent.type(await view.findByRole('textbox', { name: 'Name' }), ' edited')
  fireEvent.click(view.getByRole('button', { name: 'Cancel' }))
  expect(view.getByRole('heading', { hidden: true }).textContent).toBe('Duplicate view')
  fireEvent.click(view.getByRole('button', { name: 'Edit B', hidden: true }))
  expect((view.getByRole('textbox', { name: 'Name' }) as HTMLInputElement).value).toBe('Beta')
})
