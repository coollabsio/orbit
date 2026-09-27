import { expect, test } from 'bun:test'
import { fireEvent, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { renderWithProviders, savedView } from '../testUtils'
import type { ViewStateController } from '../useViewState'
import { defaultViewState, emptyFilter } from '../viewState'
import { ViewChanges } from './ViewChanges'

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

function renderChanges(controller: ViewStateController, onSaveAsNew: (options?: { instant?: boolean }) => void = () => {}) {
  return renderWithProviders(<ViewChanges controller={controller} onSaveAsNew={onSaveAsNew} />)
}

test('editors get Reset, Save as new view, and Update view; Ctrl+S updates', async () => {
  const { controller, calls } = fakeController({ dirty: true })
  let asNew = 0
  const { view } = renderChanges(controller, () => { asNew += 1 })
  const bar = view.getByRole('group', { name: 'Unsaved view changes' })
  expect(bar.classList.contains('animate-view-bar-enter')).toBe(true)
  expect(within(bar).getAllByRole('button').map((button) => button.textContent)).toEqual(['Reset', 'Save as new view', 'Update view'])
  await userEvent.click(within(bar).getByRole('button', { name: 'Reset' }))
  await userEvent.click(within(bar).getByRole('button', { name: 'Save as new view' }))
  fireEvent.keyDown(document, { key: 's', ctrlKey: true })
  await waitFor(() => expect(calls).toEqual(['discard', 'save']))
  expect(asNew).toBe(1)
})

test('non-editors get Reset and Save as new view, and Cmd+S opens it', async () => {
  let asNew = 0
  const { controller } = fakeController({ dirty: true, canEdit: false })
  const { view } = renderChanges(controller, () => { asNew += 1 })
  expect(view.queryAllByRole('button', { name: 'Update view' })).toHaveLength(0)
  await userEvent.click(view.getByRole('button', { name: 'Save as new view' }))
  fireEvent.keyDown(document, { key: 's', metaKey: true })
  expect(asNew).toBe(2)
})

test('a version conflict offers Overwrite and Reload', async () => {
  const { controller, calls } = fakeController({ dirty: true, conflict: true })
  const { view } = renderChanges(controller)
  expect(await view.findByRole('alertdialog')).toBeTruthy()
  expect(view.getByText('This view changed since you opened it')).toBeTruthy()
  await userEvent.click(view.getByRole('button', { name: 'Overwrite' }))
  await waitFor(() => expect(calls).toContain('overwrite'))
  await userEvent.click(view.getByRole('button', { name: 'Reload' }))
  await waitFor(() => expect(calls).toContain('discard'))
})

test('a held or repeated Cmd+S saves once while the save is in flight', async () => {
  let release: (() => void) | undefined
  let saves = 0
  const { controller } = fakeController({
    dirty: true,
    save: () => {
      saves += 1
      return new Promise<void>((resolve) => { release = resolve })
    },
  })
  renderChanges(controller)
  fireEvent.keyDown(document, { key: 's', metaKey: true })
  fireEvent.keyDown(document, { key: 's', metaKey: true, repeat: true })
  fireEvent.keyDown(document, { key: 's', metaKey: true })
  expect(saves).toBe(1)

  release?.()
  await waitFor(() => {
    fireEvent.keyDown(document, { key: 's', metaKey: true })
    expect(saves).toBe(2)
  })
})

test('Cmd+S opens Save as new view without animation; the button animates', async () => {
  const opened: Array<{ instant?: boolean } | undefined> = []
  const { controller } = fakeController({ dirty: true, canEdit: false })
  const { view } = renderChanges(controller, (options) => opened.push(options))
  fireEvent.keyDown(document, { key: 's', ctrlKey: true })
  await userEvent.click(view.getByRole('button', { name: 'Save as new view' }))
  expect(opened.map((options) => options?.instant ?? false)).toEqual([true, false])
})
