import { expect, test } from 'bun:test'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import { CATEGORY_LABEL, CATEGORY_ORDER, isClosedCategory, sortStatuses, taskViewCreateDefaults } from './taskMeta'

const now = new Date('2026-09-22T18:30:00.000Z')

test('My tasks assigns a new task to the current user', () => {
  expect(taskViewCreateDefaults('mine', 'user-1', now)).toEqual({ assignee_ids: ['user-1'] })
})

test('date views give a new task a due date that matches the active view', () => {
  expect(taskViewCreateDefaults('overdue', 'user-1', now)).toEqual({ due_at: '2026-09-21T23:59:59.999Z' })
  expect(taskViewCreateDefaults('due_soon', 'user-1', now)).toEqual({ due_at: '2026-09-22T12:00:00.000Z' })
})

test('This week selects the full local Monday-through-Sunday range', () => {
  const defaults = taskViewCreateDefaults('current_week', 'user-1', now)
  const start = new Date(defaults.due_start_at!)
  const end = new Date(defaults.due_at!)

  expect(start.getDay()).toBe(1)
  expect(start.getHours()).toBe(0)
  expect(end.getDay()).toBe(0)
  expect(end.getHours()).toBe(12)
})

test('the all-tasks view does not add personal defaults', () => {
  expect(taskViewCreateDefaults(undefined, 'user-1', now)).toEqual({})
})

test('duplicate is the last workflow category and counts as closed', () => {
  expect(CATEGORY_ORDER).toEqual(['unstarted', 'started', 'completed', 'cancelled', 'duplicate'])
  expect(CATEGORY_LABEL.duplicate).toBe('Duplicate')
  expect(isClosedCategory('duplicate')).toBe(true)
  expect(isClosedCategory('cancelled')).toBe(true)
  expect(isClosedCategory('completed')).toBe(true)
  expect(isClosedCategory('started')).toBe(false)
  expect(isClosedCategory(undefined)).toBe(false)
  const status = (id: string, category: TaskStatusDef['category']): TaskStatusDef => ({
    id, projectId: 'p1', name: id, description: '', color: '#888', category, position: 0, version: 1,
  })
  expect(sortStatuses([status('dup', 'duplicate'), status('todo', 'unstarted')]).map((s) => s.id)).toEqual(['todo', 'dup'])
})
