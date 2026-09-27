import { expect, test } from 'bun:test'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import { CATEGORY_LABEL, CATEGORY_ORDER, isClosedCategory, sortStatuses, statusKeyOf } from './taskMeta'

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

test('status keys lowercase ASCII only, like the server filter', () => {
  const status = (name: string): TaskStatusDef => ({
    id: 's1', projectId: 'p1', name, description: '', color: '#888', category: 'started', position: 0, version: 1,
  })
  expect(statusKeyOf(status(' In Review '))).toBe('started:in review')
  expect(statusKeyOf(status('Überprüfung'))).toBe('started:Überprüfung')
})
