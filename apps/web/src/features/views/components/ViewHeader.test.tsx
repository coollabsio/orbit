import { afterEach, expect, test } from 'bun:test'
import { fireEvent, waitFor, within } from '@testing-library/react'
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

function renderHeader(controller: ViewStateController, handlers: Partial<{ onSaveAsNew: () => void; onDeleted: () => void }> = {}) {
  return renderWithProviders(
    <div>
      <ViewHeader
        workspaceId="alpha"
        controller={controller}
        onSaveAsNew={handlers.onSaveAsNew ?? (() => {})}
        onEdit={() => {}}
        onDuplicate={() => {}}
        onDeleted={handlers.onDeleted ?? (() => {})}
      />
    </div>,
  )
}

test('a clean view shows its name and favorite star, with no save controls', async () => {
  const requests = stubFetch((request) => (request.method === 'GET' ? Response.json([]) : new Response(null, { status: 204 })))
  const { view } = renderHeader(fakeController().controller)
  expect(view.getByRole('heading', { name: 'Launch' })).toBeTruthy()
  expect(view.queryByRole('group', { name: 'Unsaved view changes' })).toBeNull()
  await userEvent.click(view.getByRole('button', { name: 'Add to favorites' }))
  await waitFor(() => expect(requests.some((request) => request.method === 'PUT' && request.path.endsWith('/views/view-1/favorite'))).toBe(true))
})

test('editors get Discard and Save, and Ctrl+S saves', async () => {
  const { controller, calls } = fakeController({ dirty: true })
  const { view } = renderHeader(controller)
  const bar = view.getByRole('group', { name: 'Unsaved view changes' })
  expect(bar.classList.contains('animate-view-bar-enter')).toBe(true)
  await userEvent.click(within(bar).getByRole('button', { name: 'Discard' }))
  fireEvent.keyDown(document, { key: 's', ctrlKey: true })
  await waitFor(() => expect(calls).toEqual(['discard', 'save']))
})

test('non-editors get Save as new view, and Cmd+S opens it', async () => {
  let asNew = 0
  const { controller } = fakeController({ dirty: true, canEdit: false })
  const { view } = renderHeader(controller, { onSaveAsNew: () => { asNew += 1 } })
  expect(view.queryByRole('button', { name: 'Save' })).toBeNull()
  await userEvent.click(view.getByRole('button', { name: 'Save as new view' }))
  fireEvent.keyDown(document, { key: 's', metaKey: true })
  expect(asNew).toBe(2)
})

test('a version conflict offers Overwrite and Reload', async () => {
  const { controller, calls } = fakeController({ dirty: true, conflict: true })
  const { view } = renderHeader(controller)
  expect(await view.findByRole('alertdialog')).toBeTruthy()
  expect(view.getByText('This view changed since you opened it')).toBeTruthy()
  await userEvent.click(view.getByRole('button', { name: 'Overwrite' }))
  await waitFor(() => expect(calls).toContain('overwrite'))
  await userEvent.click(view.getByRole('button', { name: 'Reload' }))
  await waitFor(() => expect(calls).toContain('discard'))
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
  expect(view.getByText("This view has settings Orbit can't read. Save to reset them.")).toBeTruthy()
  expect(view.getByText('View not found')).toBeTruthy()
  expect(view.getByRole('link', { name: 'Go to views' }).getAttribute('href')).toBe('/views')
})
