import { afterEach, expect, test } from 'bun:test'
import { fireEvent, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders, savedView, stubFetch } from '../testUtils'
import type { ViewStateController } from '../useViewState'
import { defaultViewState, emptyFilter } from '../viewState'
import { ViewHeader, ViewNotFound, ViewStateBanner } from './ViewHeader'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

function fakeController(overrides: Partial<ViewStateController> = {}) {
  const calls: string[] = []
  const controller: ViewStateController = {
    state: defaultViewState(),
    effective: emptyFilter(),
    setFilter: () => {},
    setDisplay: () => {},
    reset: () => {},
    dirty: false,
    canEdit: true,
    view: savedView({ id: 'view-1', name: 'Launch' }),
    stateError: null,
    conflict: false,
    save: async (options) => {
      calls.push(options?.overwrite ? 'overwrite' : 'save')
    },
    discard: () => {
      calls.push('discard')
    },
    isLoading: false,
    ...overrides,
  }
  return { controller, calls }
}

function renderHeader(controller: ViewStateController, handlers: Partial<{ onDeleted: () => void }> = {}) {
  return renderWithProviders(
    <div>
      <ViewHeader workspaceId="alpha" controller={controller} onEdit={() => {}} onDuplicate={() => {}} onDeleted={handlers.onDeleted ?? (() => {})} />
    </div>,
  )
}

test('the header shows the name and favorite star; unsaved edits are never handled here', async () => {
  const requests = stubFetch((request) => (request.method === 'GET' ? Response.json([]) : new Response(null, { status: 204 })))
  const { view } = renderHeader(fakeController({ dirty: true }).controller)
  expect(view.getByRole('heading', { name: 'Launch' })).toBeTruthy()
  expect(view.queryByRole('group', { name: 'Unsaved view changes' })).toBeNull()
  await userEvent.click(view.getByRole('button', { name: 'Add to favorites' }))
  await waitFor(() => expect(requests.some((request) => request.method === 'PUT' && request.path.endsWith('/views/view-1/favorite'))).toBe(true))
})

test('deleting from the view menu asks first, then leaves the page', async () => {
  let deleted = 0
  const requests = stubFetch((request) => (request.method === 'GET' ? Response.json([]) : new Response(null, { status: 204 })))
  const { view } = renderHeader(fakeController().controller, { onDeleted: () => { deleted += 1 } })
  fireEvent.click(view.getByRole('button', { name: 'View options' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Delete view' }))
  expect(await view.findByText('Delete view?')).toBeTruthy()
  await userEvent.click(view.getByRole('button', { name: 'Delete view' }))
  await waitFor(() => expect(deleted).toBe(1))
  expect(requests.some((request) => request.method === 'DELETE' && request.path.endsWith('/views/view-1'))).toBe(true)
})

test('explains unreadable settings and missing views', () => {
  const { view } = renderWithProviders(
    <>
      <ViewStateBanner />
      <ViewNotFound />
    </>,
  )
  expect(view.getByText("This view has settings Orbit can't read. Update the view to reset them.")).toBeTruthy()
  expect(view.getByText('View not found')).toBeTruthy()
  expect(view.getByRole('link', { name: 'Go to views' }).getAttribute('href')).toBe('/views')
})
