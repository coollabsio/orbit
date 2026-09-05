import { expect, mock, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { TaskCommentComposer } from './TaskCommentComposer'

test('comment writes announce progress and keep a visible retry after failure', () => {
  const view = render(<TaskCommentComposer placeholder="Reply" pending={false} progress={50} error="Upload failed." onSend={mock(async () => {})} />)
  fireEvent.change(view.getByPlaceholderText('Reply'), { target: { value: 'Keep this draft' } })

  expect(view.getByRole('alert').textContent).toContain('Upload failed.')
  expect(view.getByRole('status').textContent).toContain('50%')
  expect(view.getByRole('button', { name: 'Retry' })).toBeTruthy()
})
