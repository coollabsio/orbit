import { expect, test } from 'bun:test'
import type { Task } from './api/models'
import { SORT_OPTIONS, boardDropUpdates, chunkTaskUpdates, needsExhaustiveTaskList, taskApiSort } from './tasksLib'

function task(id: string, statusId: string, position: number, version: number): Task {
  return {
    id, statusId, position, version, projectId: 'project-1', title: id, description: '',
    identifier: id, priority: 'none', assigneeIds: [], creatorId: 'user-1', labels: [],
    attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [],
  }
}

test('board drops create an exhaustive integer order for the destination column', () => {
  const moving = task('moving', 'todo', 20, 2)
  const updates = boardDropUpdates(moving, [task('first', 'doing', 0, 4), task('last', 'doing', 10, 6)], 'doing', 1)

  expect(updates).toEqual([
    { id: 'first', expected_version: 4, position: 0 },
    { id: 'moving', expected_version: 2, position: 1, status_id: 'doing' },
    { id: 'last', expected_version: 6, position: 2 },
  ])
  expect(updates.every((update) => Number.isInteger(update.position))).toBeTrue()
})

test('large board reindexes stay within the 100-update server boundary', () => {
  const moving = task('moving', 'todo', 500, 2)
  const destination = Array.from({ length: 205 }, (_, index) => task(`task-${index}`, 'doing', index, 1))
  const updates = boardDropUpdates(moving, destination, 'doing', 102)
  const batches = chunkTaskUpdates(updates)

  expect(batches.map((batch) => batch.length)).toEqual([100, 100, 6])
  expect(batches.flat()).toEqual(updates)
  expect(batches.every((batch) => batch.length <= 100)).toBeTrue()
  expect(updates.every((update) => Number.isInteger(update.position))).toBeTrue()
})

test('every advertised task sort maps to a supported server sort', () => {
  expect(SORT_OPTIONS.map(({ key }) => taskApiSort(key).sort)).toEqual([
    'position', 'priority', 'created_at', 'updated_at', 'title',
  ])
})

test('cross-project status filtering exhausts pagination before filtering', () => {
  expect(needsExhaustiveTaskList(null, 'unstarted:todo')).toBeTrue()
  expect(needsExhaustiveTaskList('project-1', 'unstarted:todo')).toBeFalse()
  expect(needsExhaustiveTaskList(null, null)).toBeFalse()
})
