import { expect, mock, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { TaskCommentComposer } from './TaskCommentComposer'

test('comment writes announce progress and keep a visible retry after failure', () => {
  const view = render(<TaskCommentComposer placeholder="Reply" pending={false} progress={50} error="Upload failed." onSend={mock(async () => {})} />)
  fireEvent.change(view.getByPlaceholderText('Reply'), { target: { value: 'Keep this draft' } })

  expect(view.getByRole('alert').textContent).toContain('Upload failed.')
  expect(view.getByRole('status').textContent).toContain('50%')
  expect(view.getByRole('button', { name: 'Retry' })).toBeTruthy()
})

test('at-mentions resolve to workspace member ids', async () => {
  const sent: Array<{ body: string; ids: string[] }> = []
  const view = render(<TaskCommentComposer placeholder="Reply" pending={false} members={[{
    id: 'user-2', membershipId: 'm2', name: 'Ada', handle: 'ada', email: 'ada@orbit.test',
    role: 'Member', color: '#000', online: false, title: '', roleIds: [], version: 1,
  }]} onSend={async (body, _files, mentionedUserIds) => { sent.push({ body, ids: mentionedUserIds }) }} />)
  await userEvent.type(view.getByPlaceholderText('Reply'), 'Hey @')
  fireEvent.mouseDown(await view.findByRole('button', { name: '@Ada' }))
  fireEvent.click(view.getByRole('button', { name: 'Send' }))
  expect(sent[0]?.ids).toEqual(['user-2'])
  expect(sent[0]?.body).toContain('@Ada')
})
