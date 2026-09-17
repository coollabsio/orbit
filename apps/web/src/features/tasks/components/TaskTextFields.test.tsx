import { expect, mock, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import type { Task } from '../api/models'
import { TaskTextFields } from './TaskTextFields'

function task(version: number, title: string, description: string): Task {
  return {
    id: 'task-1', version, title, description, statusId: 'todo', position: 0,
    projectId: 'project-1', identifier: 'ORB-1', priority: 'none', assigneeIds: [],
    creatorId: 'user-1', labels: [], attachments: [], dueAt: null, createdAt: '',
    updatedAt: '', comments: [], activity: [],
  }
}

test('authoritative conflict refresh replaces visible controlled title and description drafts', () => {
  const onUpdate = mock(() => {})
  const view = render(<TaskTextFields task={task(1, 'Original', 'Old description')} onUpdate={onUpdate} />)
  fireEvent.click(view.getByText('Original'))
  fireEvent.change(view.getByLabelText('Task title'), { target: { value: 'Rejected title' } })
  fireEvent.click(view.getByText('Old description'))
  fireEvent.change(view.getByLabelText('Description'), { target: { value: 'Rejected description' } })

  view.rerender(<TaskTextFields task={task(2, 'Server title', 'Server description')} onUpdate={onUpdate} />)

  expect(view.getByText('Server title')).toBeTruthy()
  expect(view.getByText('Server description')).toBeTruthy()
})

test('web addresses in task titles and descriptions become safe links', () => {
  const view = render(
    <TaskTextFields
      task={task(1, 'Review https://example.com/change', 'Details at https://example.com/docs.')}
      onUpdate={() => {}}
    />,
  )

  const links = view.getAllByRole('link') as HTMLAnchorElement[]
  expect(links.map((link) => link.textContent)).toEqual([
    'https://example.com/change',
    'https://example.com/docs',
  ])
  expect(links.map((link) => link.href)).toEqual([
    'https://example.com/change',
    'https://example.com/docs',
  ])
  expect(links.every((link) => link.target === '_blank')).toBe(true)
  expect(links.every((link) => link.rel === 'noreferrer')).toBe(true)
})

test('plain task text stays editable when clicked', () => {
  const view = render(<TaskTextFields task={task(1, 'Editable title', 'Editable description')} onUpdate={() => {}} />)

  fireEvent.click(view.getByText('Editable title'))
  expect(view.getByLabelText('Task title')).toBeTruthy()
  fireEvent.click(view.getByText('Editable description'))
  expect(view.getByLabelText('Description')).toBeTruthy()
})

test('new untitled task opens with an empty focused title field', () => {
  const view = render(<TaskTextFields task={task(1, 'Untitled', '')} onUpdate={() => {}} />)
  const title = view.getByLabelText('Task title') as HTMLInputElement

  expect(title.value).toBe('')
  expect(title.placeholder).toBe('Task title')
  expect(document.activeElement).toBe(title)
})

test('Tab moves focus from a new task title to its description', () => {
  const view = render(<TaskTextFields task={task(1, 'Untitled', '')} onUpdate={() => {}} />)
  const title = view.getByLabelText('Task title')

  fireEvent.keyDown(title, { key: 'Tab' })

  expect(document.activeElement).toBe(view.getByLabelText('Description'))
})
