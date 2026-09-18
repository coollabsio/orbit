import { expect, mock, test } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'
import type { Task } from '../api/models'
import { ReferencedBy } from './ReferencedBy'

function task(id: string, identifier: string, overrides: Partial<Task> = {}): Task {
  return {
    id, identifier, title: identifier, descriptionJson: { type: 'doc', content: [] }, descriptionText: '',
    statusId: 's', position: 0, priority: 'none', assigneeIds: [], creatorId: 'u', projectId: 'p', labels: [],
    attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 0,
    parentId: null, subIssueTotal: 0, subIssueDone: 0, duplicateOfTaskId: null, duplicateIds: [], referencedBy: [],
    ...overrides,
  }
}

test('backlinks list task and comment sources and open the owning task', () => {
  const target = task('task-1', 'ORB-1', {
    referencedBy: [
      { sourceType: 'task', sourceId: 'task-4', sourceTaskId: 'task-4', sourceTaskIdentifier: 'ORB-4', sourceTaskTitle: 'Plan' },
      { sourceType: 'comment', sourceId: 'comment-1', sourceTaskId: 'task-7', sourceTaskIdentifier: 'ORB-7', sourceTaskTitle: 'Review' },
    ],
  })
  const onOpen = mock((_taskId: string) => {})
  const view = render(<ReferencedBy task={target} onOpen={onOpen} />)

  expect(view.getByText('Referenced by')).toBeTruthy()
  expect(view.getByRole('button', { name: 'ORB-4' })).toBeTruthy()
  expect(view.getByText('Plan')).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Comment on ORB-7' }))
  expect(onOpen).toHaveBeenCalledWith('task-7')
})

test('nothing renders without backlinks', () => {
  const view = render(<ReferencedBy task={task('task-1', 'ORB-1')} onOpen={() => {}} />)
  expect(view.queryByText('Referenced by')).toBeNull()
})
