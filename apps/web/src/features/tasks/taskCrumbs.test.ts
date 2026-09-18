import { expect, test } from 'bun:test'
import type { Task } from './api/models'
import { taskCrumbs } from './taskCrumbs'

function task(id: string, identifier: string, parentId: string | null): Task {
  return {
    id, identifier, title: identifier, descriptionJson: { type: 'doc', content: [] }, descriptionText: '',
    statusId: 's', position: 0, priority: 'none', assigneeIds: [], creatorId: 'u', projectId: 'p', labels: [],
    attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 0,
    parentId, subIssueTotal: 0, subIssueDone: 0, duplicateOfTaskId: null, duplicateIds: [], referencedBy: [],
  }
}

test('a sub-issue shows its parent before itself', () => {
  const parent = task('task-9', 'ORB-9', null)
  const child = task('task-12', 'ORB-12', 'task-9')
  expect(taskCrumbs(child, [parent, child])).toEqual([
    { label: 'ORB-9', to: '/tasks/task-9' },
    { label: 'ORB-12' },
  ])
})

test('a child whose parent is in the trash renders an unlinked marker, not a broken link', () => {
  const child = task('task-12', 'ORB-12', 'task-9')
  expect(taskCrumbs(child, [child])).toEqual([
    { label: 'In trash', muted: true },
    { label: 'ORB-12' },
  ])
})

test('a parent that is still loading is a placeholder, not "In trash"', () => {
  const child = task('task-12', 'ORB-12', 'task-9')
  expect(taskCrumbs(child, [child], true)).toEqual([
    { label: '…', muted: true },
    { label: 'ORB-12' },
  ])
})

test('a root task has no parent crumb', () => {
  const root = task('task-1', 'ORB-1', null)
  expect(taskCrumbs(root, [root])).toEqual([{ label: 'ORB-1' }])
})
