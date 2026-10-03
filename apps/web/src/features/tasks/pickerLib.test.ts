import { expect, test } from 'bun:test'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import { statusPickerOptions, toggleAssignee } from './pickerLib'

const status = (id: string, projectId: string, category: TaskStatusDef['category'], position: number): TaskStatusDef =>
  ({ id, projectId, name: id, description: '', color: '#888', category, position, version: 1 })
const statuses = [
  status('dup', 'p1', 'duplicate', 3),
  status('todo', 'p1', 'unstarted', 0),
  status('other', 'p2', 'unstarted', 0),
  status('done', 'p1', 'completed', 1),
]

test('the status menu lists only the task\'s project, and Duplicate only where a canonical task can be picked', () => {
  expect(statusPickerOptions(statuses, 'p1', false).map((s) => s.id)).toEqual(['todo', 'done'])
  expect(statusPickerOptions(statuses, 'p1', true).map((s) => s.id)).toEqual(['todo', 'done', 'dup'])
})

test('toggling an assignee adds or removes only that user', () => {
  expect(toggleAssignee(['a', 'b'], 'a')).toEqual(['b'])
  expect(toggleAssignee(['a'], 'c')).toEqual(['a', 'c'])
})
