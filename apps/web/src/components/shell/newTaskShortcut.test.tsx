import { expect, mock, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { NEW_TASK_KEY, useNewTaskShortcut } from './newTaskShortcut'

function Host({ onNewTask }: { onNewTask: () => void }) {
  useNewTaskShortcut(onNewTask)
  return (
    <>
      <input aria-label="Title" />
      <div data-testid="note" contentEditable suppressContentEditableWarning />
    </>
  )
}

test('Linear-style c creates a task from anywhere in the document', () => {
  const onNewTask = mock(() => {})
  render(<Host onNewTask={onNewTask} />)

  expect(NEW_TASK_KEY).toBe('c')
  fireEvent.keyDown(document.body, { key: 'c' })
  expect(onNewTask).toHaveBeenCalledTimes(1)
})

test('the shortcut stays out of the way of typing and browser chords', () => {
  const onNewTask = mock(() => {})
  const view = render(<Host onNewTask={onNewTask} />)

  fireEvent.keyDown(view.getByLabelText('Title'), { key: 'c' })
  fireEvent.keyDown(view.getByTestId('note'), { key: 'c' })
  fireEvent.keyDown(document.body, { key: 'c', metaKey: true })
  fireEvent.keyDown(document.body, { key: 'c', ctrlKey: true })
  fireEvent.keyDown(document.body, { key: 'c', altKey: true })
  fireEvent.keyDown(document.body, { key: 'v' })

  expect(onNewTask).not.toHaveBeenCalled()
})

test('the listener is removed when the shell unmounts', () => {
  const onNewTask = mock(() => {})
  const view = render(<Host onNewTask={onNewTask} />)
  view.unmount()

  fireEvent.keyDown(document.body, { key: 'c' })
  expect(onNewTask).not.toHaveBeenCalled()
})

test('the shell wires the shortcut to the same route the deleted New menu used', async () => {
  const source = await Bun.file(new URL('./AppShell.tsx', import.meta.url)).text()

  expect(source).toContain('useNewTaskShortcut(newTask)')
  expect(source).toContain("navigate('/tasks?new=1')")
})
