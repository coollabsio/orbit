import { expect, test } from 'bun:test'
import { moveToastMessage, projectUpdates } from './useMoveToProject'

test('a move sends each task that is not in the project yet, with its version and no status', () => {
  const tasks = [
    { id: 'a', version: 3, projectId: 'eng' },
    { id: 'b', version: 1, projectId: 'ops' },
  ]
  expect(projectUpdates(tasks, 'ops')).toEqual([{ id: 'a', expected_version: 3, project_id: 'ops' }])
  expect(projectUpdates(tasks.slice(1), 'ops')).toEqual([])
})

test('the toast names one moved task by its new identifier, several by count', () => {
  const ops = { key: 'OPS', name: 'Operations' }
  expect(moveToastMessage(ops, [{ id: 'task-a', number: 3 }])).toBe('Moved to OPS-3')
  expect(moveToastMessage(ops, [{ id: 'task-a', number: 3 }, { id: 'task-b', number: 4 }])).toBe('Moved 2 tasks to Operations')
})
