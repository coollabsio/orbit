import { expect, test } from 'bun:test'
import type { Task } from '@/features/tasks/api/models'
import { canNestOn, nestZoneAt, planNestDrop } from './nestDrop'
import { buildTaskTree } from './taskTree'

const t = (id: string, parentTaskId: string | null = null): Task => ({
  id, parentTaskId, identifier: id, title: id, description: '', statusId: 'todo', position: 0, priority: 'none', assigneeIds: [],
  projectId: 'project-1', labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
})

test('rows split 25% before, 50% inside, 25% after', () => {
  const rect = { top: 100, height: 40 }
  expect(nestZoneAt(rect, 109)).toBe('before')
  expect(nestZoneAt(rect, 110)).toBe('inside')
  expect(nestZoneAt(rect, 130)).toBe('inside')
  expect(nestZoneAt(rect, 131)).toBe('after')
})

test('a task never nests into itself or any of its sub-issues', () => {
  const tasks = [t('a'), t('b', 'a'), t('c', 'b'), t('d')]
  expect(canNestOn(tasks, 'a', 'a')).toBe(false)
  expect(canNestOn(tasks, 'a', 'c')).toBe(false)
  expect(canNestOn(tasks, 'c', 'a')).toBe(true)
  expect(canNestOn(tasks, 'a', 'd')).toBe(true)
})

test('inside makes the target the parent unless it already is', () => {
  const tasks = [t('a'), t('b', 'a'), t('d')]
  expect(planNestDrop({ tasks, tree: null, dragId: 'd', target: tasks[0]!, zone: 'inside', manual: true })).toEqual({ kind: 'parent', parentId: 'a', placement: null })
  expect(planNestDrop({ tasks, tree: null, dragId: 'b', target: tasks[0]!, zone: 'inside', manual: true })).toEqual({ kind: 'invalid' })
  expect(planNestDrop({ tasks, tree: null, dragId: 'a', target: tasks[1]!, zone: 'inside', manual: true })).toEqual({ kind: 'invalid' })
})

test('edges of a nested row make a sibling; root rows, flat lists and boards pass to the group zone', () => {
  const tasks = [t('a'), t('b', 'a'), t('c', 'a'), t('d')]
  const tree = buildTaskTree(tasks)
  const [, b, c, d] = tasks as [Task, Task, Task, Task]
  expect(planNestDrop({ tasks, tree, dragId: 'd', target: b, zone: 'after', manual: true }))
    .toEqual({ kind: 'parent', parentId: 'a', placement: { siblings: [b, c], index: 1 } })
  expect(planNestDrop({ tasks, tree, dragId: 'd', target: b, zone: 'before', manual: false })).toEqual({ kind: 'parent', parentId: 'a', placement: null })
  expect(planNestDrop({ tasks, tree, dragId: 'c', target: b, zone: 'before', manual: false })).toEqual({ kind: 'invalid' })
  expect(planNestDrop({ tasks, tree, dragId: 'a', target: b, zone: 'after', manual: true })).toEqual({ kind: 'invalid' })
  expect(planNestDrop({ tasks, tree, dragId: 'b', target: d, zone: 'before', manual: true })).toEqual({ kind: 'pass' })
  expect(planNestDrop({ tasks, tree: null, dragId: 'd', target: b, zone: 'after', manual: true })).toEqual({ kind: 'pass' })
})

test('in manual order, sub-issues in another status or project only take a new parent, never a new position', () => {
  const tasks = [t('a'), { ...t('b', 'a'), statusId: 'doing' }, t('c', 'a'), t('d')]
  const tree = buildTaskTree(tasks)
  const [, b, c, d] = tasks as [Task, Task, Task, Task]
  expect(planNestDrop({ tasks, tree, dragId: 'd', target: c, zone: 'before', manual: true })).toEqual({ kind: 'parent', parentId: 'a', placement: null })
  expect(planNestDrop({ tasks, tree, dragId: 'c', target: b, zone: 'before', manual: true })).toEqual({ kind: 'invalid' })
  expect(planNestDrop({ tasks: [...tasks.slice(0, 3), { ...d, projectId: 'project-2' }], tree, dragId: 'd', target: c, zone: 'after', manual: true }))
    .toEqual({ kind: 'parent', parentId: 'a', placement: null })
})
