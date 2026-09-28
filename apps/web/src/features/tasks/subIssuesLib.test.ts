import { expect, test } from 'bun:test'
import { ApiProblem } from '@/api/problem'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import { breadcrumbParts, descendantCount, parentErrorMessage, parentPickerTitle, parentToastMessage, subIssueDefaults, trashConfirmDescription, trashedSubIssuesLabel } from './subIssuesLib'

const problem = (code: string) => new ApiProblem({ type: 'about:blank', title: 'Unprocessable', status: 422, detail: 'x', code, instance: '/', request_id: 'r' } as never)

test('parent toasts name the task and the new parent, or say it is no longer a sub-issue', () => {
  expect(parentToastMessage(['ORB-31'], 'ORB-12')).toBe('ORB-31 is now a sub-issue of ORB-12')
  expect(parentToastMessage(['ORB-31', 'ORB-32'], 'ORB-12')).toBe('2 tasks are now sub-issues of ORB-12')
  expect(parentToastMessage(['ORB-31'], null)).toBe('ORB-31 is no longer a sub-issue')
  expect(parentToastMessage(['ORB-31', 'ORB-32'], null)).toBe('2 tasks are no longer sub-issues')
})

test('server refusals explain themselves', () => {
  expect(parentErrorMessage(problem('parent_cycle'))).toBe('A task can’t be a sub-issue of itself or of its own sub-issues.')
  expect(parentErrorMessage(problem('parent_invalid'))).toBe('That parent task isn’t available anymore.')
  expect(parentErrorMessage(new Error('boom'))).toBe('Couldn’t change the parent.')
})

test('the parent picker title names the task or the count', () => {
  expect(parentPickerTitle('ORB-31')).toBe('Set parent of ORB-31…')
  expect(parentPickerTitle(3)).toBe('Set parent of 3 tasks…')
})

const statuses: TaskStatusDef[] = [
  { id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#888', category: 'unstarted', position: 0, version: 1 },
  { id: 'done', projectId: 'project-1', name: 'Done', description: '', color: '#4cb782', category: 'completed', position: 1, version: 1 },
]
const parent = { id: 'task-0012', projectId: 'project-1', priority: 'high' as const, assigneeIds: ['user-2'] }

test('a new sub-issue takes the parent\'s project, priority and parent, the default status, and no labels', () => {
  expect(subIssueDefaults(parent, { statuses, currentUserId: 'user-1', siblings: [] })).toEqual({
    project_id: 'project-1', status_id: 'todo', priority: 'high', assignee_ids: [], parent_task_id: 'task-0012',
  })
  expect(subIssueDefaults({ ...parent, projectId: 'project-9' }, { statuses, currentUserId: 'user-1', siblings: [] })).toBeNull()
})

test('the parent\'s assignee is copied only for its own assignee or when every sub-issue shares it', () => {
  const defaults = (currentUserId: string, siblings: Array<{ assigneeIds: string[] }>) =>
    subIssueDefaults(parent, { statuses, currentUserId, siblings })!.assignee_ids
  expect(defaults('user-2', [])).toEqual(['user-2'])
  expect(defaults('user-1', [{ assigneeIds: ['user-2'] }, { assigneeIds: ['user-2'] }])).toEqual(['user-2'])
  expect(defaults('user-1', [{ assigneeIds: ['user-2'] }, { assigneeIds: [] }])).toEqual([])
})

test('more than three ancestors collapse the middle ones', () => {
  expect(breadcrumbParts(['a', 'b', 'c'])).toEqual({ head: ['a', 'b', 'c'], hidden: [], tail: [] })
  expect(breadcrumbParts(['a', 'b', 'c', 'd', 'e'])).toEqual({ head: ['a'], hidden: ['b', 'c', 'd'], tail: ['e'] })
})

test('trash rows count the sub-issues trashed with a parent', () => {
  expect(trashedSubIssuesLabel(0)).toBeNull()
  expect(trashedSubIssuesLabel(1)).toBe('+1 sub-issue')
  expect(trashedSubIssuesLabel(3)).toBe('+3 sub-issues')
})

test('the trash confirmation counts every sub-issue going with the task, or only claims the direct ones', () => {
  const levels: Record<string, Array<{ id: string; sub_issue_count: number }>> = {
    root: [{ id: 'a', sub_issue_count: 2 }, { id: 'b', sub_issue_count: 0 }],
    a: [{ id: 'a1', sub_issue_count: 0 }, { id: 'a2', sub_issue_count: 0 }],
  }
  expect(descendantCount(2, (id) => levels[id], 'root')).toEqual({ total: 4, exact: true })
  // level "a" is not loaded: only the direct count is known
  expect(descendantCount(2, (id) => (id === 'a' ? undefined : levels[id]), 'root')).toEqual({ total: 2, exact: false })
  expect(descendantCount(0, () => undefined, 'root')).toEqual({ total: 0, exact: true })

  expect(trashConfirmDescription({ total: 0, exact: true })).toBe('You can restore this task from trash later.')
  expect(trashConfirmDescription({ total: 1, exact: true })).toBe('This also moves 1 sub-issue to trash. You can restore them from trash later.')
  expect(trashConfirmDescription({ total: 4, exact: true })).toBe('This also moves 4 sub-issues to trash. You can restore them from trash later.')
  expect(trashConfirmDescription({ total: 2, exact: false })).toBe('This also moves its 2 sub-issues and everything nested under them to trash. You can restore them from trash later.')
})
