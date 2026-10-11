import { expect, mock, test } from 'bun:test'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { TaskCommentComposer } from './TaskCommentComposer'
import { NO_MEMBER_ABILITIES } from '@/features/workspaces/models'

test('comment writes announce progress and keep a visible retry after failure', () => {
  const view = render(<TaskCommentComposer placeholder="Reply" pending={false} progress={50} error="Upload failed." onSend={mock(async () => {})} />)
  fireEvent.change(view.getByPlaceholderText('Reply'), { target: { value: 'Keep this draft' } })

  expect(view.getByRole('alert').textContent).toContain('Upload failed.')
  expect(view.getByRole('status').textContent).toContain('50%')
  expect(view.getByRole('button', { name: 'Retry' })).toBeTruthy()
})

test('an at-mention writes the member name into the text, which the server reads', async () => {
  const sent: string[] = []
  const view = render(<TaskCommentComposer placeholder="Reply" pending={false} members={[{
    id: 'user-2', membershipId: 'm2', name: 'Ada', handle: 'ada', email: 'ada@orbit.test',
    role: 'Member', color: '#000', title: '', roleIds: [], can: NO_MEMBER_ABILITIES, version: 1,
  }]} onSend={async (body) => { sent.push(body) }} />)
  await userEvent.type(view.getByPlaceholderText('Reply'), 'Hey @')
  fireEvent.mouseDown(await view.findByRole('button', { name: '@Ada' }))
  fireEvent.click(view.getByRole('button', { name: 'Send' }))
  expect(sent[0]).toBe('Hey @Ada ')
})

test('Enter adds a new line and only the Send button sends', async () => {
  const onSend = mock(async () => {})
  const view = render(<TaskCommentComposer placeholder="Reply" pending={false} onSend={onSend} />)
  const field = view.getByPlaceholderText('Reply') as HTMLTextAreaElement
  await userEvent.type(field, 'First{Enter}Second')

  expect(onSend).not.toHaveBeenCalled()
  expect(field.value).toBe('First\nSecond')
})

test('a second click while sending does not send the comment again', async () => {
  const onSend = mock(() => new Promise<void>(() => {}))
  const view = render(<TaskCommentComposer placeholder="Reply" pending={false} onSend={onSend} />)
  await userEvent.type(view.getByPlaceholderText('Reply'), 'Once')
  await userEvent.dblClick(view.getByRole('button', { name: 'Send' }))

  expect(onSend).toHaveBeenCalledTimes(1)
})

test('the rich composer sends its markdown, and pasted markdown becomes rich text', async () => {
  const sent: string[] = []
  const view = render(<TaskCommentComposer rich placeholder="Leave a comment…" pending={false} onSend={async (body) => { sent.push(body) }} />)
  const editors = () => view.container.querySelectorAll('[data-slot="markdown-editor"]').length
  await waitFor(() => { if (editors() !== 1) throw new Error('the editor did not open') })
  // nothing to send yet
  expect((view.getByRole('button', { name: 'Send' }) as HTMLButtonElement).disabled).toBe(true)
  const tiptap = (view.container.querySelector('.ProseMirror') as HTMLElement & { editor: { commands: { insertContent: (text: string) => boolean } } }).editor
  tiptap.commands.insertContent('Ship it')
  await waitFor(() => { if ((view.getByRole('button', { name: 'Send' }) as HTMLButtonElement).disabled) throw new Error('no text yet') })
  // Mod+Enter sends
  fireEvent.keyDown(view.container.querySelector('[data-slot="markdown-editor"]')!, { key: 'Enter', ctrlKey: true })
  await waitFor(() => { if (sent.length !== 1) throw new Error('not sent') })
  expect(sent[0]).toBe('Ship it')

  // there is no mode to choose
  expect(view.queryAllByRole('button', { name: 'Markdown' })).toHaveLength(0)
  const paste = (text: string) => {
    const data = new DataTransfer()
    data.setData('text/plain', text)
    const event = new Event('paste', { bubbles: true, cancelable: true }) as Event & { clipboardData: DataTransfer }
    event.clipboardData = data
    view.container.querySelector('.ProseMirror')!.dispatchEvent(event)
  }
  const sendPasted = async (text: string) => {
    await waitFor(() => { if (view.container.querySelectorAll('[data-slot="markdown-editor"]').length !== 1) throw new Error('no editor') })
    paste(text)
    await waitFor(() => { if ((view.getByRole('button', { name: 'Send' }) as HTMLButtonElement).disabled) throw new Error('no text yet') })
    const count = sent.length
    fireEvent.click(view.getByRole('button', { name: 'Send' }))
    await waitFor(() => { if (sent.length !== count + 1) throw new Error('not sent') })
    return sent[count]
  }
  // markdown is parsed: it comes back as the same markdown, and the editor holds a list
  expect(await sendPasted('Steps:\n\n- one\n- **two**')).toBe('Steps:\n\n- one\n- **two**')
  // text that markdown parsing would damage is pasted as it is
  expect(await sendPasted('Use Vec<String> here')).toContain('Vec<String>')
})
