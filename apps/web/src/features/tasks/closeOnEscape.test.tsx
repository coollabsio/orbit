import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { shouldCloseTaskOnKey } from './closeOnEscape'

function capture(target: Element, init: KeyboardEventInit, preventFirst = false) {
  let result: boolean | undefined
  const listener = (event: Event) => { result = shouldCloseTaskOnKey(event as KeyboardEvent) }
  if (preventFirst) target.addEventListener('keydown', (event) => event.preventDefault(), { once: true })
  document.addEventListener('keydown', listener, { once: true })
  fireEvent.keyDown(target, init)
  return result
}

test('Escape on the page closes the task', () => {
  expect(capture(document.body, { key: 'Escape' })).toBe(true)
})

test('an Escape already handled by a menu or editor does not also close the task', () => {
  // The @ menu handles Escape inside ProseMirror, which calls preventDefault.
  expect(capture(document.body, { key: 'Escape' }, true)).toBe(false)
})

test('Escape from a text field leaves the field first, like Linear', () => {
  const view = render(
    <>
      <input aria-label="Title" />
      <div data-testid="editor" contentEditable suppressContentEditableWarning />
    </>,
  )
  expect(capture(view.getByLabelText('Title'), { key: 'Escape' })).toBe(false)
  expect(capture(view.getByTestId('editor'), { key: 'Escape' })).toBe(false)
})

test('other keys never close the task, and an open dialog owns Escape', () => {
  expect(capture(document.body, { key: 'Enter' })).toBe(false)
  const dialog = document.createElement('div')
  dialog.setAttribute('role', 'dialog')
  dialog.setAttribute('aria-modal', 'true')
  document.body.append(dialog)
  expect(capture(document.body, { key: 'Escape' })).toBe(false)
  dialog.remove()
})
