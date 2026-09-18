import { expect, mock, test } from 'bun:test'
import { act, fireEvent, render } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import { TaskCommentComposer } from './TaskCommentComposer'

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return (
    <QueryClientProvider client={client}>
      <MemoryRouter>{children}</MemoryRouter>
    </QueryClientProvider>
  )
}

test('comment writes announce progress and keep a visible retry after failure', () => {
  const view = render(
    <TaskCommentComposer
      placeholder="Reply"
      pending={false}
      progress={50}
      error="Upload failed."
      workspaceId="w"
      members={[]}
      onSend={mock(async () => {})}
    />,
    { wrapper },
  )

  expect(view.getByRole('alert').textContent).toContain('Upload failed.')
  expect(view.getByRole('status').textContent).toContain('50%')
  expect(view.getByRole('button', { name: 'Retry' })).toBeTruthy()
})

test('the composer is a rich text surface, not a textarea', () => {
  const view = render(
    <TaskCommentComposer placeholder="Reply" pending={false} workspaceId="w" members={[]} onSend={mock(async () => {})} />,
    { wrapper },
  )

  expect(view.container.querySelector('textarea')).toBeNull()
  expect(view.getByLabelText('Reply')).toBeTruthy()
})

test('the send hint names the Cmd/Ctrl+Enter shortcut', () => {
  const view = render(
    <TaskCommentComposer placeholder="Reply" pending={false} workspaceId="w" members={[]} onSend={mock(async () => {})} />,
    { wrapper },
  )

  expect(view.getByText(/enter to/i).textContent?.toLowerCase()).toContain('send')
})

test('attachment-only comments can still be sent with an empty document', async () => {
  const sent: { document: unknown; files: File[] }[] = []
  const view = render(
    <TaskCommentComposer
      placeholder="Reply"
      pending={false}
      workspaceId="w"
      members={[]}
      onSend={async (bodyJson, files) => {
        sent.push({ document: bodyJson, files })
      }}
    />,
    { wrapper },
  )

  const input = view.getByLabelText('Attach comment files') as HTMLInputElement
  const file = new File(['x'], 'shot.png', { type: 'image/png' })
  fireEvent.change(input, { target: { files: [file] } })
  // Sending resolves asynchronously and then clears the composer.
  await act(async () => {
    fireEvent.click(view.getByRole('button', { name: 'Send' }))
  })

  expect(sent).toHaveLength(1)
  expect(sent[0].files[0].name).toBe('shot.png')
  expect(view.queryByText('shot.png')).toBeNull()
})

test('an empty composer with no files cannot send', () => {
  const onSend = mock(async () => {})
  const view = render(
    <TaskCommentComposer placeholder="Reply" pending={false} workspaceId="w" members={[]} onSend={onSend} />,
    { wrapper },
  )

  const send = view.getByRole('button', { name: 'Send' }) as HTMLButtonElement
  expect(send.disabled).toBe(true)
  fireEvent.click(send)
  expect(onSend).not.toHaveBeenCalled()
})
