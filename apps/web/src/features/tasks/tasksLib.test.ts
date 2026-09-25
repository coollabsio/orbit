import { expect, test } from 'bun:test'
import type { Task } from '@/features/tasks/api/models'
import { quickSearchTasks } from './tasksLib'

function task(id: string, statusId: string, position: number, version: number): Task {
  return {
    id, statusId, position, version, projectId: 'project-1', title: id, description: '',
    identifier: id, priority: 'none', assigneeIds: [], creatorId: 'user-1', labels: [],
    attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [],
  }
}

test('quick search matches title, description and identifier in memory', () => {
  const title = task('ORB-1', 'todo', 0, 1)
  title.title = 'Ship release'
  const description = task('ORB-2', 'todo', 1, 1)
  description.description = 'Prepare launch notes'

  expect(quickSearchTasks([title, description], ' Launch ').map(({ id }) => id)).toEqual(['ORB-2'])
  expect(quickSearchTasks([title, description], 'orb-1').map(({ id }) => id)).toEqual(['ORB-1'])
  const all = [title, description]
  expect(quickSearchTasks(all, '  ')).toBe(all)
})
