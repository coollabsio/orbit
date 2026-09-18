import { expect, mock, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { SIDEBAR_TOGGLE_KEY, useSidebarToggleShortcut } from './sidebarShortcut'

function Host({ onToggle }: { onToggle: () => void }) {
  useSidebarToggleShortcut(onToggle)
  return (
    <>
      <input aria-label="Title" />
      <div data-testid="note" contentEditable suppressContentEditableWarning />
    </>
  )
}

test('Linear-style bracket toggles the sidebar from anywhere in the document', () => {
  const onToggle = mock(() => {})
  render(<Host onToggle={onToggle} />)

  expect(SIDEBAR_TOGGLE_KEY).toBe('[')
  fireEvent.keyDown(document.body, { key: '[' })
  expect(onToggle).toHaveBeenCalledTimes(1)
})

test('the shortcut stays out of the way of typing and browser chords', () => {
  const onToggle = mock(() => {})
  const view = render(<Host onToggle={onToggle} />)

  fireEvent.keyDown(view.getByLabelText('Title'), { key: '[' })
  fireEvent.keyDown(view.getByTestId('note'), { key: '[' })
  fireEvent.keyDown(document.body, { key: '[', metaKey: true })
  fireEvent.keyDown(document.body, { key: '[', ctrlKey: true })
  fireEvent.keyDown(document.body, { key: '[', altKey: true })
  fireEvent.keyDown(document.body, { key: ']' })

  expect(onToggle).not.toHaveBeenCalled()
})

test('the listener is removed when the shell unmounts', () => {
  const onToggle = mock(() => {})
  const view = render(<Host onToggle={onToggle} />)
  view.unmount()

  fireEvent.keyDown(document.body, { key: '[' })
  expect(onToggle).not.toHaveBeenCalled()
})
