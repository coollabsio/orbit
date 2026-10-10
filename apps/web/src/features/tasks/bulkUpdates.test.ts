import { expect, test } from 'bun:test'
import type { Milestone } from '@/features/tasks/api/milestones'
import type { Task } from '@/features/tasks/api/models'
import { milestoneUpdates, sharedProjectMilestones } from './bulkUpdates'

const task = (id: string, overrides: Partial<Task> = {}): Task => ({
  id, identifier: id, title: id, description: '', statusId: 's', position: 0, priority: 'none', assigneeIds: [], projectId: 'p1',
  labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 3, ...overrides,
})
const milestone = (id: string, projectId: string) => ({ id, project_id: projectId }) as Milestone

test('milestone updates leave out the tasks that already have the value', () => {
  const tasks = [task('a'), task('b', { milestoneId: 'm1' })]
  expect(milestoneUpdates(tasks, 'm1')).toEqual([{ id: 'a', expected_version: 3, milestone_id: 'm1' }])
  expect(milestoneUpdates(tasks, null)).toEqual([{ id: 'b', expected_version: 3, milestone_id: null }])
})

test('milestones are offered only when every task is in one project', () => {
  const milestones = [milestone('m1', 'p1'), milestone('m2', 'p2')]
  expect(sharedProjectMilestones([task('a'), task('b')], milestones).map((item) => item.id)).toEqual(['m1'])
  expect(sharedProjectMilestones([task('a'), task('b', { projectId: 'p2' })], milestones)).toEqual([])
  expect(sharedProjectMilestones([], milestones)).toEqual([])
})
