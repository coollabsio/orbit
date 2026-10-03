import { expect, test } from 'bun:test'
import type { LabelRecord, ProjectRecord } from '@/api/generated/types.gen'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import { createDefaultsFromFilter, dropUpdate, groupTasks, type GroupContext } from './grouping'
import { effectiveFilter, emptyFilter, type FilterGroup } from './viewState'
import { NO_MEMBER_ABILITIES } from '@/features/workspaces/models'

const status = (id: string, projectId: string, name: string, category: TaskStatusDef['category'], position: number): TaskStatusDef => ({
  id, projectId, name, description: '', color: '#888888', category, position, version: 1,
})
const member = (id: string, name: string): User => ({
  id, membershipId: `membership-${id}`, name, handle: id, email: `${id}@orbit.test`, role: 'Member',
  color: '#888888', title: '', roleIds: [], can: NO_MEMBER_ABILITIES, version: 1,
})
const label = (id: string, name: string): LabelRecord => ({ id, name, color: '#888888', version: 1, workspace_id: 'workspace-1' })
const project = (id: string, name: string): ProjectRecord => ({
  id, name, key: name.slice(0, 3).toUpperCase(), color: '#888888', created_at: '', updated_at: '', version: 1, workspace_id: 'workspace-1',
  auto_close_parent: true, auto_close_sub_issues: true,
})
function task(id: string, overrides: Partial<Task> = {}): Task {
  return {
    id, identifier: id, title: id, description: '', statusId: 'p1-todo', position: 0, priority: 'none',
    assigneeIds: [], projectId: 'p1', labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '',
    comments: [], activity: [], version: 2, ...overrides,
  }
}

const ctx: GroupContext = {
  statuses: [
    status('p1-done', 'p1', 'Done', 'completed', 0),
    status('p1-doing', 'p1', 'In Progress', 'started', 0),
    status('p1-todo', 'p1', 'Todo', 'unstarted', 0),
    status('p2-todo', 'p2', 'Todo', 'unstarted', 0),
    status('p2-review', 'p2', 'Review', 'started', 1),
  ],
  members: [member('user-zed', 'Zed'), member('user-bob', 'Bob'), member('user-ann', 'Ann')],
  labels: [label('label-ui', 'UI'), label('label-bug', 'Bug')],
  projects: [project('p1', 'Launch'), project('p2', 'Alpha')],
  currentUserId: 'user-zed',
  showEmpty: false,
}
const summary = (groups: ReturnType<typeof groupTasks>) => groups.map((group) => [group.key, group.label, group.tasks.map(({ id }) => id)])

test('status groups follow workflow order, merge same-named statuses across projects and hide empty groups', () => {
  const tasks = [task('a', { statusId: 'p1-doing' }), task('b', { statusId: 'p2-todo', projectId: 'p2' }), task('c')]
  expect(summary(groupTasks(tasks, 'status', ctx))).toEqual([
    ['status:unstarted:todo', 'Todo', ['b', 'c']],
    ['status:started:in progress', 'In Progress', ['a']],
  ])
  expect(groupTasks(tasks, 'status', { ...ctx, showEmpty: true }).map((group) => group.value)).toEqual([
    'unstarted:todo', 'started:in progress', 'started:review', 'completed:done',
  ])
})

test('priority groups run urgent to none', () => {
  const tasks = [task('low', { priority: 'low' }), task('urgent', { priority: 'urgent' }), task('none')]
  expect(summary(groupTasks(tasks, 'priority', ctx))).toEqual([
    ['priority:urgent', 'Urgent', ['urgent']],
    ['priority:low', 'Low', ['low']],
    ['priority:none', 'No priority', ['none']],
  ])
  expect(groupTasks([], 'priority', { ...ctx, showEmpty: true }).map((group) => group.value)).toEqual(['urgent', 'high', 'medium', 'low', 'none'])
})

test('assignee groups put the current user first, then A-Z, then No assignee; shared tasks appear in each', () => {
  const tasks = [
    task('a', { assigneeIds: ['user-bob', 'user-zed'] }),
    task('b'),
    task('c', { assigneeIds: ['user-removed'] }),
    task('d', { assigneeIds: ['user-ann', 'user-ann'] }),
  ]
  expect(summary(groupTasks(tasks, 'assignee', ctx))).toEqual([
    ['assignee:user-zed', 'Zed', ['a']],
    ['assignee:user-ann', 'Ann', ['d']],
    ['assignee:user-bob', 'Bob', ['a']],
    ['assignee:none', 'No assignee', ['b', 'c']],
  ])
  expect(groupTasks([task('a', { assigneeIds: ['user-ann'] })], 'assignee', { ...ctx, showEmpty: true }).map((group) => group.value)).toEqual([
    'user-zed', 'user-ann', 'user-bob', null,
  ])
})

test('label groups run A-Z then No label; project groups run A-Z with no empty project group', () => {
  const tasks = [task('a', { labels: ['label-ui', 'label-bug'] }), task('b', { projectId: 'p2', statusId: 'p2-todo' }), task('c', { labels: ['label-gone'] })]
  expect(summary(groupTasks(tasks, 'label', ctx))).toEqual([
    ['label:label-bug', 'Bug', ['a']],
    ['label:label-ui', 'UI', ['a']],
    ['label:none', 'No label', ['b', 'c']],
  ])
  expect(summary(groupTasks(tasks, 'project', ctx))).toEqual([
    ['project:p2', 'Alpha', ['b']],
    ['project:p1', 'Launch', ['a', 'c']],
  ])
})

test('no grouping yields one group, even when empty', () => {
  expect(summary(groupTasks([task('a'), task('b')], 'none', ctx))).toEqual([['all', 'All tasks', ['a', 'b']]])
  expect(summary(groupTasks([], 'none', ctx))).toEqual([['all', 'All tasks', []]])
})

test('status drops map the key to the task project, or refuse when it has no such status', () => {
  expect(dropUpdate(task('a'), 'status', 'unstarted:todo', 'started:in progress', ctx)).toEqual({
    taskId: 'a', patch: { expected_version: 2, status_id: 'p1-doing' },
  })
  expect(dropUpdate(task('b', { projectId: 'p2', statusId: 'p2-todo' }), 'status', 'unstarted:todo', 'started:in progress', ctx)).toEqual({
    error: 'No matching status in Alpha',
  })
  expect(dropUpdate(task('a'), 'status', 'unstarted:todo', 'unstarted:todo', ctx)).toBeNull()
})

test('priority drops set the priority', () => {
  expect(dropUpdate(task('a'), 'priority', 'none', 'urgent', ctx)).toEqual({ taskId: 'a', patch: { expected_version: 2, priority: 'urgent' } })
})

test('assignee and label drops swap the source value for the target', () => {
  const shared = task('a', { assigneeIds: ['user-bob', 'user-zed'], labels: ['label-ui'] })
  expect(dropUpdate(shared, 'assignee', 'user-bob', 'user-ann', ctx)).toEqual({ taskId: 'a', patch: { expected_version: 2, assignee_ids: ['user-zed', 'user-ann'] } })
  expect(dropUpdate(shared, 'assignee', 'user-bob', 'user-zed', ctx)).toEqual({ taskId: 'a', patch: { expected_version: 2, assignee_ids: ['user-zed'] } })
  expect(dropUpdate(shared, 'assignee', 'user-bob', null, ctx)).toEqual({ taskId: 'a', patch: { expected_version: 2, assignee_ids: ['user-zed'] } })
  expect(dropUpdate(task('b'), 'assignee', null, 'user-ann', ctx)).toEqual({ taskId: 'b', patch: { expected_version: 2, assignee_ids: ['user-ann'] } })
  expect(dropUpdate(shared, 'label', 'label-ui', 'label-bug', ctx)).toEqual({ taskId: 'a', patch: { expected_version: 2, label_ids: ['label-bug'] } })
  expect(dropUpdate(shared, 'label', 'label-ui', null, ctx)).toEqual({ taskId: 'a', patch: { expected_version: 2, label_ids: [] } })
  expect(dropUpdate(task('b'), 'label', null, null, ctx)).toBeNull()
})

test('project drops and ungrouped drops change nothing', () => {
  expect(dropUpdate(task('a'), 'project', 'p1', 'p2', ctx)).toBeNull()
  expect(dropUpdate(task('a'), 'none', null, null, ctx)).toBeNull()
})

const tuesdayEvening = new Date('2026-09-22T18:30:00.000Z')
const target = (targetProjectId: string | null) => ({ ...ctx, targetProjectId })

test('preset pages give new tasks the defaults that keep them in the page', () => {
  expect(createDefaultsFromFilter(effectiveFilter(emptyFilter(), { preset: 'mine' }), target('p1'), tuesdayEvening)).toEqual({
    project_id: 'p1', assignee_ids: ['user-zed'],
  })
  expect(createDefaultsFromFilter(effectiveFilter(emptyFilter(), { preset: 'overdue' }), target('p1'), tuesdayEvening)).toEqual({
    project_id: 'p1', due_at: '2026-09-21T23:59:59.999Z',
  })
  expect(createDefaultsFromFilter(effectiveFilter(emptyFilter(), { preset: 'due_soon' }), target('p1'), tuesdayEvening)).toEqual({
    project_id: 'p1', due_at: '2026-09-22T12:00:00.000Z',
  })
  expect(createDefaultsFromFilter(effectiveFilter(emptyFilter(), { preset: 'current_week' }), target('p1'), tuesdayEvening)).toEqual({
    project_id: 'p1', due_start_at: '2026-09-21T00:00:00.000Z', due_at: '2026-09-27T12:00:00.000Z',
  })
  expect(createDefaultsFromFilter(effectiveFilter(emptyFilter(), { preset: 'my_week' }), target('p1'), tuesdayEvening)).toEqual({
    project_id: 'p1', assignee_ids: ['user-zed'], due_start_at: '2026-09-21T00:00:00.000Z', due_at: '2026-09-27T12:00:00.000Z',
  })
  expect(createDefaultsFromFilter(emptyFilter(), target('p1'), tuesdayEvening)).toEqual({ project_id: 'p1' })
  expect(createDefaultsFromFilter(emptyFilter(), target(null), tuesdayEvening)).toEqual({})
})

test('single-value root conditions become defaults; the status resolves in the target project', () => {
  const filter: FilterGroup = { op: 'and', children: [
    { field: 'project', operator: 'is', value: ['p2'] },
    { field: 'status', operator: 'is', value: ['started:review'] },
    { field: 'priority', operator: 'is', value: ['high'] },
    { field: 'label', operator: 'includes_any', value: ['label-bug'] },
    { field: 'label', operator: 'includes_all', value: ['label-ui', 'label-bug'] },
    { field: 'assignee', operator: 'is', value: ['user-ann'] },
  ] }
  expect(createDefaultsFromFilter(filter, target('p1'), tuesdayEvening)).toEqual({
    project_id: 'p2', status_id: 'p2-review', priority: 'high', label_ids: ['label-bug', 'label-ui'], assignee_ids: ['user-ann'],
  })
  // the status key has no match in the target project: the caller falls back to the default status
  expect(createDefaultsFromFilter({ op: 'and', children: [{ field: 'status', operator: 'is', value: ['started:review'] }] }, target('p1'), tuesdayEvening)).toEqual({ project_id: 'p1' })
})

test('multi-value, negative, nested, OR-rooted and unknown conditions give no defaults', () => {
  const ignored: FilterGroup = { op: 'and', children: [
    { field: 'priority', operator: 'is', value: ['high', 'urgent'] },
    { field: 'priority', operator: 'is_not', value: ['low'] },
    { field: 'label', operator: 'includes_any', value: ['label-ui', 'label-bug'] },
    { field: 'assignee', operator: 'is', value: ['user-removed'] },
    { field: 'project', operator: 'is', value: ['project-deleted'] },
    { field: 'label', operator: 'excludes', value: ['label-bug'] },
    { op: 'and', children: [{ field: 'priority', operator: 'is', value: ['urgent'] }] },
  ] }
  expect(createDefaultsFromFilter(ignored, target('p1'), tuesdayEvening)).toEqual({ project_id: 'p1' })
  expect(createDefaultsFromFilter({ op: 'or', children: [{ field: 'priority', operator: 'is', value: ['urgent'] }] }, target('p1'), tuesdayEvening)).toEqual({ project_id: 'p1' })
  expect(createDefaultsFromFilter(effectiveFilter(emptyFilter(), { preset: 'mine' }), { ...target('p1'), currentUserId: '' }, tuesdayEvening)).toEqual({ project_id: 'p1' })
})
