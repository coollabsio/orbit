import { expect, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { SideSheet, SideSheetContent } from './SideSheet'

test('Escape in a text field or already handled stays inside the sheet; otherwise it closes it', () => {
  const changes: boolean[] = []
  const view = render(
    <SideSheet open onOpenChange={(open) => changes.push(open)}>
      <SideSheetContent aria-label="Thread">
        <button>Plain</button>
        <input aria-label="Title" />
        <div aria-label="Editor" contentEditable />
        <button onKeyDown={(event) => event.preventDefault()}>Handled</button>
      </SideSheetContent>
    </SideSheet>,
  )
  const title = view.getByLabelText('Title')
  title.focus()
  fireEvent.keyDown(title, { key: 'Escape' })
  fireEvent.keyDown(view.getByLabelText('Editor'), { key: 'Escape' })
  fireEvent.keyDown(view.getByRole('button', { name: 'Handled' }), { key: 'Escape' })
  expect(changes).toEqual([])
  const plain = view.getByRole('button', { name: 'Plain' })
  plain.focus()
  fireEvent.keyDown(plain, { key: 'Escape' })
  expect(changes).toEqual([false])
})
