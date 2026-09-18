import { expect, test } from 'bun:test'
import type { Task } from '../api/models'
import { nestTasks } from './taskNesting'

function task(id: string, parentId: string | null): Task {
  return {
    id, identifier: id.toUpperCase(), title: id, descriptionJson: { type: 'doc', content: [] }, descriptionText: '',
    statusId: 'todo', position: 0, priority: 'none', assigneeIds: [], creatorId: 'u', projectId: 'p', labels: [],
    attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 0,
    parentId, subIssueTotal: 0, subIssueDone: 0, duplicateOfTaskId: null, duplicateIds: [], referencedBy: [],
  }
}

test('nestTasks groups one level of children under their root', () => {
  const nested = nestTasks([task('a', null), task('b', 'a'), task('c', null)])
  expect(nested.map((entry) => entry.root.id)).toEqual(['a', 'c'])
  expect(nested[0].children.map((child) => child.id)).toEqual(['b'])
})

test('a child whose parent is absent from the page is promoted to a root', () => {
  const nested = nestTasks([task('b', 'missing')])
  expect(nested.map((entry) => entry.root.id)).toEqual(['b'])
  expect(nested[0].children).toEqual([])
})

test('grandchildren attach to their topmost present ancestor, one level only', () => {
  const nested = nestTasks([task('a', null), task('b', 'a'), task('c', 'b')])
  expect(nested.map((entry) => entry.root.id)).toEqual(['a'])
  expect(nested[0].children.map((child) => child.id)).toEqual(['b', 'c'])
})

test('a child listed before its parent still nests, and the parent keeps its place', () => {
  const nested = nestTasks([task('b', 'a'), task('c', null), task('a', null)])
  expect(nested.map((entry) => entry.root.id)).toEqual(['a', 'c'])
  expect(nested[0].children.map((child) => child.id)).toEqual(['b'])
})

test('a cycle in the data cannot hang the walk', () => {
  const nested = nestTasks([task('a', 'b'), task('b', 'a')])
  expect(nested.flatMap((entry) => [entry.root.id, ...entry.children.map((child) => child.id)]).sort()).toEqual(['a', 'b'])
})
