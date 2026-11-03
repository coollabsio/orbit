import { ApiProblem } from '@orbit/api/problem'
import { expect, test } from 'bun:test'

import type { LabelRecord, ProjectRecord, StatusRecord, TaskRecord } from './orbit'
import type { TaskRelationRecord } from './task-api'
import {
  attachmentPath,
  bodyWithFiles,
  dueDateLabel,
  dueOn,
  dueWeek,
  fileSize,
  groupRelations,
  labelChoices,
  matchingLabelIds,
  nextLabelColor,
  optimisticTask,
  parentErrorMessage,
  relatedTaskIds,
  relationPickerTitle,
  sourceHost,
  sourceValue,
  splitInlineFiles,
  subIssueBody,
  taskCandidates,
  timeOf,
  toggleId,
} from './tasks'

const label = (id: string, name: string, project_id = 'p1', color = '#5e6ad2'): LabelRecord => ({ id, name, project_id, color, version: 1, workspace_id: 'w' })
const task = (id: string, fields: Partial<TaskRecord> = {}): TaskRecord =>
  ({ id, title: id, description: '', number: 1, project_id: 'p1', status_id: 'todo', priority: 'none', assignee_ids: [], label_ids: [], duplicate_of: null, ...fields }) as TaskRecord
const relation = (id: string, type: TaskRelationRecord['type'], direction: TaskRelationRecord['direction'], other: string, created_at = '2026-10-01T00:00:00Z'): TaskRelationRecord => ({
  id,
  type,
  direction,
  created_at,
  task: { id: other, number: 1, project_id: 'p1', status_id: 'todo', title: other, version: 1 },
})

test('toggleId adds a missing id and removes a present one', () => {
  expect(toggleId(['a', 'b'], 'c')).toEqual(['a', 'b', 'c'])
  expect(toggleId(['a', 'b'], 'a')).toEqual(['b'])
})

test('optimisticTask shows the scalar fields at once and leaves parent and duplicate to the server', () => {
  const before = task('t', { priority: 'none', parent_task_id: null })
  const shown = optimisticTask(before, { priority: 'urgent', label_ids: ['l1'], parent_task_id: 'p', duplicate_of_id: 'd' })
  expect(shown.priority).toBe('urgent')
  expect(shown.label_ids).toEqual(['l1'])
  expect(shown.parent_task_id).toBe(null)
  expect('duplicate_of_id' in shown).toBe(false)
})

test('source: host for the chip, and an empty field clears it', () => {
  expect(sourceHost('https://github.com/coollabsio/orbit/issues/1')).toBe('github.com')
  expect(sourceValue('  https://x.dev/a  ')).toBe('https://x.dev/a')
  expect(sourceValue('   ')).toBe(null)
})

test('due dates: one day at a time keeps no start; a week is Monday midnight to Sunday at the time', () => {
  const day = dueOn(new Date(2026, 9, 23), '17:30')
  expect(day.start).toBe(null)
  expect(timeOf(new Date(day.end))).toBe('17:30')
  expect(new Date(day.end).getDate()).toBe(23)

  const friday = new Date(2026, 9, 23, 15, 0)
  const thisWeek = dueWeek(0, '09:00', friday)
  const nextWeek = dueWeek(1, '09:00', friday)
  const local = (iso: string) => {
    const date = new Date(iso)
    return `${date.getMonth() + 1}/${date.getDate()} ${timeOf(date)}`
  }
  expect([local(thisWeek.start!), local(thisWeek.end)]).toEqual(['10/19 00:00', '10/25 09:00'])
  expect([local(nextWeek.start!), local(nextWeek.end)]).toEqual(['10/26 00:00', '11/1 09:00'])
})

test('dueDateLabel: the prompt, a range, or a date with its time', () => {
  expect(dueDateLabel(null)).toBe('Set due date')
  expect(dueDateLabel(new Date(2026, 9, 25, 9).toISOString(), new Date(2026, 9, 19).toISOString(), 'en-US')).toBe('Oct 19, 2026 – Oct 25, 2026')
  expect(dueDateLabel(new Date(2026, 9, 23, 9).toISOString(), null, 'en-US')).toBe('Oct 23, 2026, 09:00 AM')
})

test('labelChoices: the task labels first, then by name; a new name becomes a Create row', () => {
  const labels = [label('l1', 'Bug'), label('l2', 'API'), label('l3', 'Docs'), label('x', 'Other project', 'p2')]
  expect(labelChoices(labels, 'p1', '', ['l3']).rows.map((item) => item.name)).toEqual(['Docs', 'API', 'Bug'])
  expect(labelChoices(labels, 'p1', 'bu', [])).toEqual({ rows: [labels[0]!], create: 'bu' })
  expect(labelChoices(labels, 'p1', ' BUG ', []).create).toBe(null)
  expect(labelChoices(labels, 'p1', 'Other project', []).create).toBe('Other project')
})

test('nextLabelColor picks the palette colour the project uses least', () => {
  expect(nextLabelColor([])).toBe('#5e6ad2')
  expect(nextLabelColor([{ color: '#5e6ad2' }, { color: '#26b5ce' }])).toBe('#4cb782')
})

test('matchingLabelIds keeps own labels and maps another project’s label by name', () => {
  const labels = [label('a1', 'Bug', 'pa'), label('b1', 'bug', 'pb'), label('b2', 'Bug', 'pb'), label('a2', 'Only A', 'pa')]
  expect(matchingLabelIds(labels, ['a1', 'a2'], 'pb')).toEqual(['b2'])
  expect(matchingLabelIds(labels, ['b1'], 'pb')).toEqual(['b1'])
})

test('relations group in the web order, oldest first, from the viewed task', () => {
  const relations = [
    relation('r1', 'related', 'outgoing', 'C', '2026-10-03T00:00:00Z'),
    relation('r2', 'blocks', 'incoming', 'A'),
    relation('r3', 'blocks', 'outgoing', 'B'),
    relation('r4', 'related', 'incoming', 'D', '2026-10-02T00:00:00Z'),
    relation('r5', 'duplicate', 'incoming', 'E'),
  ]
  expect(groupRelations(relations).map((group) => `${group.label}: ${group.relations.map((item) => item.task.id).join(',')}`)).toEqual([
    'Blocked by: A',
    'Blocks: B',
    'Related: D,C',
    'Duplicated by: E',
  ])
  expect(relatedTaskIds(relations)).toEqual(['C', 'A', 'B', 'D', 'E'])
  expect(relationPickerTitle('blocked_by', 'ENG-3')).toBe('ENG-3 is blocked by…')
  expect(relationPickerTitle('duplicate', 'ENG-3')).toBe('Mark ENG-3 as duplicate of…')
})

test('taskCandidates matches identifier, title or description, without excluded ids or duplicates', () => {
  const projects = [{ id: 'p1', key: 'ENG' }] as ProjectRecord[]
  const tasks = [
    task('a', { number: 12, title: 'Login page' }),
    task('b', { number: 13, title: 'Signup', description: 'the login form' }),
    task('a', { number: 12, title: 'Login page' }),
    task('c', { number: 14, title: 'Login copy', status_id: 'dup' }),
    task('d', { number: 15, title: 'Other' }),
  ]
  expect(taskCandidates(tasks, projects, { query: 'login', excludeIds: [] }).map((item) => item.id)).toEqual(['a', 'b', 'c'])
  expect(taskCandidates(tasks, projects, { query: 'eng-15', excludeIds: [] }).map((item) => item.id)).toEqual(['d'])
  expect(taskCandidates(tasks, projects, { query: 'login', excludeIds: ['a'], duplicateStatusIds: ['dup'] }).map((item) => item.id)).toEqual(['b'])
})

test('parentErrorMessage speaks the server codes', () => {
  const problem = (code: string) => new ApiProblem({ type: 'about:blank', title: 'x', status: 422, detail: 'x', code, instance: '', request_id: '' } as never)
  expect(parentErrorMessage(problem('parent_cycle'))).toBe('A task can’t be a sub-issue of itself or of its own sub-issues.')
  expect(parentErrorMessage(problem('parent_invalid'))).toBe('That parent task isn’t available anymore.')
  expect(parentErrorMessage(new Error('offline'))).toBe('Couldn’t change the parent.')
})

test('subIssueBody: the parent’s project, default status and priority; its assignees only when the user is one', () => {
  const statuses = [
    { id: 'backlog', project_id: 'p1', category: 'unstarted', position: 0, name: 'Backlog' },
    { id: 'doing', project_id: 'p1', category: 'started', position: 1, name: 'Doing' },
  ] as StatusRecord[]
  const parent = task('parent', { priority: 'high', assignee_ids: ['me', 'you'] })
  expect(subIssueBody(parent, 'Child', { statuses, userId: 'me', siblings: [] })).toEqual({
    title: 'Child',
    project_id: 'p1',
    status_id: 'backlog',
    priority: 'high',
    assignee_ids: ['me', 'you'],
    parent_task_id: 'parent',
  })
  expect(subIssueBody(parent, 'Child', { statuses, userId: 'them', siblings: [] })?.assignee_ids).toEqual([])
  expect(subIssueBody(parent, 'Child', { statuses, userId: 'them', siblings: [{ assignee_ids: ['you', 'me'] }] })?.assignee_ids).toEqual(['me', 'you'])
  expect(subIssueBody(parent, 'Child', { statuses: [], userId: 'me', siblings: [] })).toBe(null)
})

const W = '11111111-1111-1111-1111-111111111111'
const T = '22222222-2222-2222-2222-222222222222'
const A = '33333333-3333-3333-3333-333333333333'
const C = '44444444-4444-4444-4444-444444444444'

test('attachmentPath: the task or the comment download path', () => {
  expect(attachmentPath({ workspace_id: W, task_id: T, id: A, comment_id: null })).toBe(`/api/v1/workspaces/${W}/tasks/${T}/attachments/${A}/download`)
  expect(attachmentPath({ workspace_id: W, task_id: T, id: A, comment_id: C })).toBe(`/api/v1/workspaces/${W}/tasks/${T}/comments/${C}/attachments/${A}/download`)
})

test('a comment body with files round-trips through bodyWithFiles and splitInlineFiles', () => {
  const path = `/api/v1/workspaces/${W}/tasks/${T}/attachments/${A}/download`
  const body = bodyWithFiles('  Looks good  ', [
    { name: 'shot.png', path, image: true },
    { name: 'notes [v1].txt', path, image: false },
  ])
  expect(body).toBe(`Looks good\n\n![shot.png](${path})\n\n[notes \\[v1\\].txt](${path})`)
  expect(splitInlineFiles(body)).toEqual({
    text: 'Looks good',
    files: [
      { name: 'shot.png', path, image: true },
      { name: 'notes [v1].txt', path, image: false },
    ],
  })
  expect(bodyWithFiles('', [{ name: 'a.pdf', path, image: false }])).toBe(`[a.pdf](${path})`)
  expect(splitInlineFiles('See [the docs](https://example.com/a) and ![x](https://example.com/x.png)').files).toEqual([])
})

test('fileSize', () => {
  expect(fileSize(70)).toBe('70 B')
  expect(fileSize(1536)).toBe('1.5 KB')
  expect(fileSize(25 * 1024 * 1024)).toBe('25 MB')
})
