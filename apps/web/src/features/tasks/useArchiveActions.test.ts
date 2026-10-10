import { expect, test } from 'bun:test'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import { canArchive } from './useArchiveActions'

const status = (id: string, category: TaskStatusDef['category']): TaskStatusDef => ({ id, projectId: 'p1', name: id, description: '', color: '#888888', category, position: 0, version: 1 })
const task = (id: string, statusId: string, archivedAt: string | null = null) => ({ id, statusId, archivedAt }) as Task
const STATUSES = [status('todo', 'unstarted'), status('done', 'completed'), status('cancelled', 'cancelled'), status('dup', 'duplicate')]

test('only closed tasks that are not in the archive can be archived', () => {
  expect(canArchive([task('a', 'done'), task('b', 'cancelled'), task('c', 'dup')], STATUSES)).toBe(true)
  expect(canArchive([task('a', 'done'), task('b', 'todo')], STATUSES)).toBe(false)
  expect(canArchive([task('a', 'done', '2026-10-01T00:00:00Z')], STATUSES)).toBe(false)
  expect(canArchive([task('a', 'unknown')], STATUSES)).toBe(false)
  expect(canArchive([], STATUSES)).toBe(false)
})
