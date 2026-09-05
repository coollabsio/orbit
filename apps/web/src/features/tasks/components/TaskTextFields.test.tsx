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
  fireEvent.change(view.getByLabelText('Task title'), { target: { value: 'Rejected title' } })
  fireEvent.change(view.getByLabelText('Description'), { target: { value: 'Rejected description' } })

  view.rerender(<TaskTextFields task={task(2, 'Server title', 'Server description')} onUpdate={onUpdate} />)

  expect((view.getByLabelText('Task title') as HTMLInputElement).value).toBe('Server title')
  expect((view.getByLabelText('Description') as HTMLTextAreaElement).value).toBe('Server description')
})
