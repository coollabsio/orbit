import { expect, mock, test } from 'bun:test'
import { fireEvent, render, waitFor } from '@testing-library/react'
import { TaskCommentComposer } from './TaskCommentComposer'

test('Send stays disabled for an empty comment and a failed send keeps a Retry', () => {
  const onSend = mock(async () => {})
  const view = render(<TaskCommentComposer workspaceId="workspace-1" taskId="task-1" pending={false} members={[]} onSend={onSend} />)
  expect((view.getByRole('button', { name: 'Send' }) as HTMLButtonElement).disabled).toBe(true)

  view.rerender(<TaskCommentComposer workspaceId="workspace-1" taskId="task-1" pending={false} members={[]} error="Comment failed." onSend={onSend} />)
  expect(view.getByRole('alert').textContent).toContain('Comment failed.')
  expect(view.getByRole('button', { name: 'Retry' })).toBeTruthy()
  expect(onSend).not.toHaveBeenCalled()
})

test('a reply field mounts its editor only when it is used', async () => {
  const view = render(<TaskCommentComposer workspaceId="workspace-1" taskId="task-1" pending={false} members={[]} compact onSend={async () => {}} />)
  expect(view.container.querySelector('.bn-editor')).toBeNull()

  fireEvent.click(view.getByRole('button', { name: 'Leave a reply…' }))

  await waitFor(() => expect(view.container.querySelector('.bn-editor')).toBeTruthy(), { timeout: 4000 })
})
