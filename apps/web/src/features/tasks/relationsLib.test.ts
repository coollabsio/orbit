import { expect, test } from 'bun:test'
import type { TaskRelationRecord } from '@/api/generated/types.gen'
import type { Task } from '@/features/tasks/api/models'
import {
  ADD_RELATION_OPTIONS, duplicateErrorMessage, duplicateToastMessage, groupRelations, pickerCandidates, pickerTitle,
  relatedTaskIds,
} from './relationsLib'

const relation = (id: string, type: string, direction: string, createdAt = '2026-09-23T10:00:00Z') => ({
  id, type, direction, created_at: createdAt,
  task: { id: `task-${id}`, project_id: 'p1', title: id, status_id: 'todo' },
}) as TaskRelationRecord

const task = (id: string, title: string, overrides: Partial<Task> = {}): Task => ({
  id, identifier: `ORB-${id.slice(-4).toUpperCase()}`, title, description: '', statusId: 'todo', position: 0, priority: 'none',
  assigneeIds: [], projectId: 'p1', labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '',
  comments: [], activity: [], version: 1, duplicateOf: null, blocked: false, ...overrides,
})

test('relations group in a fixed order from the viewed task’s side; unknown types are skipped', () => {
  const groups = groupRelations([
    relation('r1', 'related', 'incoming'),
    relation('r2', 'duplicate', 'incoming'),
    relation('r3', 'blocks', 'outgoing'),
    relation('r4', 'blocks', 'incoming', '2026-09-23T11:00:00Z'),
    relation('r5', 'blocks', 'incoming', '2026-09-23T09:00:00Z'),
    relation('r6', 'duplicate', 'outgoing'),
    relation('r7', 'similar', 'outgoing'),
  ])
  expect(groups.map((group) => [group.label, group.relations.map((r) => r.id)])).toEqual([
    ['Duplicate of', ['r6']],
    ['Blocked by', ['r5', 'r4']],
    ['Blocks', ['r3']],
    ['Related', ['r1']],
    ['Duplicated by', ['r2']],
  ])
  expect(relatedTaskIds([relation('r1', 'related', 'incoming'), relation('r7', 'similar', 'outgoing')])).toEqual(['task-r1'])
})

test('picker titles and toast copy follow the spec', () => {
  expect(pickerTitle('duplicate', 'ORB-3F2A')).toBe('Mark ORB-3F2A as duplicate of…')
  expect(pickerTitle('duplicate', 4)).toBe('Mark 4 tasks as duplicate of…')
  expect(pickerTitle('blocks', 'ORB-3F2A')).toBe('ORB-3F2A blocks…')
  expect(pickerTitle('blocked_by', 'ORB-3F2A')).toBe('ORB-3F2A is blocked by…')
  expect(pickerTitle('related', 'ORB-3F2A')).toBe('Relate ORB-3F2A to…')
  expect(duplicateToastMessage(1, 'ORB-91C0')).toBe('Marked as duplicate of ORB-91C0')
  expect(duplicateToastMessage(4, 'ORB-91C0')).toBe('Marked 4 tasks as duplicate of ORB-91C0')
  expect(duplicateErrorMessage('mark', new Error('Target is a duplicate.'))).toBe("Couldn't mark as duplicate. Target is a duplicate.")
  expect(duplicateErrorMessage('unmark', 'weird')).toBe("Couldn't unmark as duplicate.")
  expect(ADD_RELATION_OPTIONS.map((option) => option.label)).toEqual(['Blocks…', 'Blocked by…', 'Related to…', 'Mark as duplicate of…'])
})

test('picker candidates dedupe, exclude, skip duplicates on request and match identifiers', () => {
  const self = task('task-3f2a', 'Self')
  const canonical = task('task-91c0', 'Login fails on Safari')
  const duplicate = task('task-0b9e', 'Safari login loop', { duplicateOf: { id: 'task-91c0', projectId: 'p1', title: 'x' } })
  const all = [canonical, self, duplicate, canonical]
  expect(pickerCandidates({ tasks: all, query: '', excludeIds: [self.id], excludeDuplicates: true }).map((t) => t.id)).toEqual(['task-91c0'])
  expect(pickerCandidates({ tasks: all, query: 'safari', excludeIds: [], excludeDuplicates: false }).map((t) => t.id)).toEqual(['task-91c0', 'task-0b9e'])
  expect(pickerCandidates({ tasks: all, query: 'orb-91', excludeIds: [], excludeDuplicates: false }).map((t) => t.id)).toEqual(['task-91c0'])
  expect(pickerCandidates({ tasks: all, query: '', excludeIds: [], excludeDuplicates: false, limit: 1 })).toHaveLength(1)
})
