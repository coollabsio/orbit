import { expect, test } from 'bun:test'
import type { Task } from '@/features/tasks/api/models'
import { SORT_OPTIONS, boardDropUpdates, filterTasks, needsExhaustiveTaskList, taskApiSort } from './tasksLib'

function task(id: string, statusId: string, position: number, version: number): Task {
  return {
    id, statusId, position, version, projectId: 'project-1', title: id, description: '',
    identifier: id, priority: 'none', assigneeIds: [], creatorId: 'user-1', labels: [],
    attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [],
  }
}

test('board drops update only the affected integer position slots', () => {
  const moving = task('moving', 'todo', 20, 2)
  const updates = boardDropUpdates(moving, [task('first', 'doing', 0, 4), task('last', 'doing', 10, 6)], 'doing', 1)

  expect(updates).toEqual([
    { id: 'moving', expected_version: 2, position: 10, status_id: 'doing' },
    { id: 'last', expected_version: 6, position: 11 },
  ])
  expect(updates.every((update) => Number.isInteger(update.position))).toBeTrue()
})

test('large board columns produce a bounded set when the drop affects at most 100 tasks', () => {
  const destination = Array.from({ length: 205 }, (_, index) => task(`task-${index}`, 'doing', index, 1))
  const updates = boardDropUpdates(task('moving', 'todo', 500, 2), destination, 'doing', 195)

  expect(updates).toHaveLength(11)
  expect(updates[0]).toEqual({ id: 'moving', expected_version: 2, position: 195, status_id: 'doing' })
  expect(updates.at(-1)).toEqual({ id: 'task-204', expected_version: 1, position: 205 })
  expect(updates.every((update) => Number.isInteger(update.position))).toBeTrue()
})

test('an in-column move reuses only changed integer position slots', () => {
  const destination = Array.from({ length: 150 }, (_, index) => task(`task-${index}`, 'doing', index * 10, 1))
  const moving = destination[120]!
  const updates = boardDropUpdates(moving, destination, 'doing', 110)

  expect(updates).toHaveLength(11)
  expect(updates.map((update) => update.position)).toEqual(destination.slice(110, 121).map((item) => item.position))
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

test('task search filters title description and identifier entirely in memory', () => {
  const title = task('ORB-1', 'todo', 0, 1)
  title.title = 'Ship release'
  const description = task('ORB-2', 'todo', 1, 1)
  description.description = 'Prepare launch notes'
  const filters = { currentUserId: 'user-1', projectId: null, statusKey: null, assigneeId: null, statuses: [], search: 'launch' }

  expect(filterTasks([title, description], filters).map(({ id }) => id)).toEqual(['ORB-2'])
  expect(filterTasks([title, description], { ...filters, search: 'orb-1' }).map(({ id }) => id)).toEqual(['ORB-1'])
})

test('task filters match label and priority', () => {
  const urgent = task('ORB-1', 'todo', 0, 1)
  urgent.priority = 'urgent'
  urgent.labels = ['bug']
  const low = task('ORB-2', 'todo', 1, 1)
  low.priority = 'low'
  low.labels = ['feature']
  const filters = { currentUserId: 'user-1', projectId: null, statusKey: null, assigneeId: null, statuses: [] }

  expect(filterTasks([urgent, low], { ...filters, labelId: 'bug' }).map(({ id }) => id)).toEqual(['ORB-1'])
  expect(filterTasks([urgent, low], { ...filters, priority: 'low' }).map(({ id }) => id)).toEqual(['ORB-2'])
  expect(filterTasks([urgent, low], { ...filters, labelId: 'feature', priority: 'urgent' })).toEqual([])
})

test('task filters match tasks without an assignee', () => {
  const unassigned = task('ORB-1', 'todo', 0, 1)
  const assigned = task('ORB-2', 'todo', 1, 1)
  assigned.assigneeIds = ['user-1']
  const filters = { currentUserId: 'user-1', projectId: null, statusKey: null, assigneeId: null, statuses: [], unassigned: true }

  expect(filterTasks([unassigned, assigned], filters).map(({ id }) => id)).toEqual(['ORB-1'])
})
