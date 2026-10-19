import { expect, test } from 'bun:test'
import type { LabelRecord } from '@/api/generated/types.gen'
import type { Task } from '@/features/tasks/api/models'
import { everyTaskHasLabel, labelChoices, labelToggleUpdates } from './bulkUpdates'
import { matchingLabelIds } from './taskMeta'

const label = (id: string, name: string, projectId: string): LabelRecord => ({ id, name, color: '#888888', version: 1, workspace_id: 'w', project_id: projectId })
const task = (id: string, projectId: string, labels: string[]): Task => ({
  id, identifier: id, title: id, description: '', statusId: 's', position: 0, priority: 'none', assigneeIds: [], projectId, labels,
  attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 3,
})
const labels = [label('a-bug', 'Bug', 'a'), label('b-bug', 'bug', 'b'), label('b-ui', 'UI', 'b'), label('c-docs', 'Docs', 'c')]

test('a label menu for tasks of several projects lists one label per name of those projects', () => {
  expect(labelChoices([task('1', 'a', []), task('2', 'b', [])], labels).map((item) => item.id)).toEqual(['a-bug', 'b-ui'])
})

test('a label toggle sets the label of each task own project with that name and skips projects without it', () => {
  const tasks = [task('1', 'a', ['a-bug']), task('2', 'b', []), task('3', 'c', [])]
  expect(everyTaskHasLabel(tasks, labels, labels[0]!)).toBe(false)
  expect(labelToggleUpdates(tasks, labels, labels[0]!)).toEqual([
    { id: '1', expected_version: 3, label_ids: ['a-bug'] },
    { id: '2', expected_version: 3, label_ids: ['b-bug'] },
  ])
  const labelled = [task('1', 'a', ['a-bug']), task('2', 'b', ['b-bug', 'b-ui']), task('3', 'c', [])]
  expect(everyTaskHasLabel(labelled, labels, labels[0]!)).toBe(true)
  expect(labelToggleUpdates(labelled, labels, labels[0]!)).toEqual([
    { id: '1', expected_version: 3, label_ids: [] },
    { id: '2', expected_version: 3, label_ids: ['b-ui'] },
  ])
})

test('labels follow by name into another project', () => {
  expect(matchingLabelIds(labels, ['a-bug'], 'b')).toEqual(['b-bug'])
  expect(matchingLabelIds(labels, ['a-bug'], 'c')).toEqual([])
  expect(matchingLabelIds(labels, ['b-bug', 'b-ui'], 'b')).toEqual(['b-bug', 'b-ui'])
})

test('a project with both "Bug" and "bug" keeps each one apart', () => {
  const both = [...labels, label('b-Bug', 'Bug', 'b')]
  expect(matchingLabelIds(both, ['b-bug'], 'b')).toEqual(['b-bug'])
  // a-bug is named "Bug": the exact name wins over "bug"
  expect(matchingLabelIds(both, ['a-bug'], 'b')).toEqual(['b-Bug'])
})
