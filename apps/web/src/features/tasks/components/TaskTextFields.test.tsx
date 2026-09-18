import { expect, mock, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { Task } from '../api/models'
import { TaskTextFields } from './TaskTextFields'

const doc = (text: string) => ({
  type: 'doc' as const,
  content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
})

function task(version: number, title: string, text: string): Task {
  return {
    id: 'task-1', version, title, descriptionJson: doc(text), descriptionText: text,
    statusId: 'todo', position: 0, projectId: 'project-1', identifier: 'ORB-1',
    priority: 'none', assigneeIds: [], creatorId: 'user-1', labels: [], attachments: [],
    dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [],
  }
}

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return (
    <QueryClientProvider client={client}>
      <MemoryRouter>{children}</MemoryRouter>
    </QueryClientProvider>
  )
}

test('the title is still a plain input and commits on blur', () => {
  const onUpdate = mock((_update: unknown) => {})
  const view = render(
    <TaskTextFields task={task(1, 'Original', 'notes')} workspaceId="w" members={[]} onUpdate={onUpdate} />,
    { wrapper },
  )

  fireEvent.click(view.getByText('Original'))
  const title = view.getByLabelText('Task title') as HTMLInputElement
  expect(title.tagName).toBe('INPUT')
  fireEvent.change(title, { target: { value: 'Renamed' } })
  fireEvent.blur(title)

  expect(onUpdate).toHaveBeenCalledWith({ title: 'Renamed' })
})

test('the description renders its document read-only until it is focused', () => {
  const view = render(
    <TaskTextFields task={task(1, 'Original', 'existing notes')} workspaceId="w" members={[]} onUpdate={() => {}} />,
    { wrapper },
  )

  expect(view.getByText('existing notes')).toBeTruthy()
  expect(view.container.querySelector('.editor-view')).toBeTruthy()
})

test('an authoritative refresh replaces the visible title draft', () => {
  const onUpdate = mock((_update: unknown) => {})
  const view = render(
    <TaskTextFields task={task(1, 'Original', 'old')} workspaceId="w" members={[]} onUpdate={onUpdate} />,
    { wrapper },
  )
  fireEvent.click(view.getByText('Original'))
  fireEvent.change(view.getByLabelText('Task title'), { target: { value: 'Rejected title' } })

  view.rerender(
    <TaskTextFields task={task(2, 'Server title', 'server notes')} workspaceId="w" members={[]} onUpdate={onUpdate} />,
  )

  expect(view.getByText('Server title')).toBeTruthy()
  expect(view.getByText('server notes')).toBeTruthy()
})

test('a new untitled task opens with an empty focused title field', () => {
  const view = render(
    <TaskTextFields task={task(1, 'Untitled', '')} workspaceId="w" members={[]} onUpdate={() => {}} />,
    { wrapper },
  )
  const title = view.getByLabelText('Task title') as HTMLInputElement

  expect(title.value).toBe('')
  expect(title.placeholder).toBe('Task title')
  expect(document.activeElement).toBe(title)
})

test('the empty description shows its placeholder prompt', () => {
  const view = render(
    <TaskTextFields task={task(1, 'Bare', '')} workspaceId="w" members={[]} onUpdate={() => {}} />,
    { wrapper },
  )

  expect(view.getByText('Add description… (paste or drop images and files)')).toBeTruthy()
})

test('web addresses in task titles become safe links', () => {
  const view = render(
    <TaskTextFields task={task(1, 'Review https://example.com/change', '')} workspaceId="w" members={[]} onUpdate={() => {}} />,
    { wrapper },
  )

  const link = view.getByRole('link') as HTMLAnchorElement
  expect(link.href).toBe('https://example.com/change')
  expect(link.target).toBe('_blank')
})

test('following a link in the description does not open the editor', () => {
  const linked = {
    ...task(1, 'Linked', 'docs'),
    descriptionJson: {
      type: 'doc' as const,
      content: [{ type: 'paragraph', content: [{ type: 'text', text: 'docs', marks: [{ type: 'link', attrs: { href: 'https://example.com/docs' } }] }] }],
    },
  }
  const view = render(<TaskTextFields task={linked} workspaceId="w" members={[]} onUpdate={() => {}} />, { wrapper })

  // Keep happy-dom from actually opening the page; propagation is what is under test.
  view.container.addEventListener('click', (event) => event.preventDefault(), true)
  fireEvent.click(view.getByRole('link', { name: 'docs' }))

  expect(view.container.querySelector('.editor-view')).toBeTruthy()
  expect(view.container.querySelector('.editor-shell')).toBeNull()
})

test('clicking the description swaps the read-only view for the editor', () => {
  const view = render(
    <TaskTextFields task={task(1, 'Editable', 'notes')} workspaceId="w" members={[]} onUpdate={() => {}} />,
    { wrapper },
  )

  fireEvent.click(view.getByText('notes'))

  expect(view.container.querySelector('.editor-view')).toBeNull()
  expect(view.container.querySelector('.editor-shell')).toBeTruthy()
})
