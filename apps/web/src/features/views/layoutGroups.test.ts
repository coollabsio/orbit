import { describe, expect, test } from 'bun:test'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import type { GroupContext } from './grouping'
import {
  ALL_TASKS_KEY, acceptsDrop, boardGrid, canDrag, cellTasks, groupAccent, groupCreateFields, listSections, placementUpdates, planDrop, valuesOf, zoneIdOf,
} from './layoutGroups'
import { DEFAULT_DISPLAY, type DisplayOptions } from './viewState'

const status = (id: string, projectId: string, name: string, category: TaskStatusDef['category'], position: number): TaskStatusDef =>
  ({ id, projectId, name, description: '', color: '#888', category, position, version: 1 })
const statuses = [
  status('todo', 'p1', 'Todo', 'unstarted', 0),
  status('doing', 'p1', 'Doing', 'started', 1),
  status('dup', 'p1', 'Duplicate', 'duplicate', 2),
  status('todo-2', 'p2', 'Todo', 'unstarted', 0),
]
const projects = [
  { id: 'p1', name: 'Launch', key: 'ORB', color: '#e0457b', version: 1 },
  { id: 'p2', name: 'Docs', key: 'DOC', color: '#26b5ce', version: 1 },
] as Project[]
const ctx: GroupContext = { statuses, members: [], labels: [], projects, currentUserId: 'u1', showEmpty: false }
const display = (patch: Partial<DisplayOptions> = {}): DisplayOptions => ({ ...DEFAULT_DISPLAY, ...patch })

function task(id: string, overrides: Partial<Task> = {}): Task {
  return {
    id, identifier: id, title: id, description: '', statusId: 'todo', position: 0, priority: 'none', assigneeIds: [],
    projectId: 'p1', labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [],
    version: 1, ...overrides,
  }
}

describe('listSections', () => {
  test('no grouping is one headerless section holding every task in order', () => {
    const tasks = [task('b'), task('a')]
    const [only, ...rest] = listSections(tasks, display({ group_by: 'none', sub_group_by: 'priority' }), ctx)
    expect(rest).toEqual([])
    expect(only!.group).toMatchObject({ key: ALL_TASKS_KEY, field: 'none', value: null })
    expect(only!.group.tasks.map((item) => item.id)).toEqual(['b', 'a'])
    expect(only!.subGroups).toBeNull()
  })

  test('sub-groups are built inside each group; a sub-group equal to the group is ignored', () => {
    const tasks = [task('a'), task('b', { priority: 'urgent' })]
    const [todo] = listSections(tasks, display({ group_by: 'status', sub_group_by: 'priority' }), ctx)
    expect(todo!.group.value).toBe('unstarted:todo')
    expect(todo!.subGroups!.map((group) => group.value)).toEqual(['urgent', 'none'])
    expect(listSections(tasks, display({ group_by: 'status', sub_group_by: 'status' }), ctx)[0]!.subGroups).toBeNull()
  })

  test('show_empty_groups adds empty groups and empty sub-groups', () => {
    const tasks = [task('a')]
    const hidden = listSections(tasks, display({ show_empty_groups: false, sub_group_by: 'priority' }), ctx)
    const shown = listSections(tasks, display({ show_empty_groups: true, sub_group_by: 'priority' }), ctx)
    expect(hidden.map((section) => section.group.value)).toEqual(['unstarted:todo'])
    expect(hidden[0]!.subGroups!.map((group) => group.value)).toEqual(['none'])
    expect(shown.map((section) => section.group.value)).toContain('started:doing')
    const doing = shown.find((section) => section.group.value === 'started:doing')!
    expect(doing.subGroups!.map((group) => [group.value, group.tasks.length])).toEqual([
      ['urgent', 0], ['high', 0], ['medium', 0], ['low', 0], ['none', 0],
    ])
  })
})

describe('drop zones', () => {
  test('values skip "none" and zone ids name every field', () => {
    expect(valuesOf({ field: 'none', value: null })).toEqual([])
    expect(zoneIdOf([])).toBe(ALL_TASKS_KEY)
    expect(zoneIdOf(valuesOf({ field: 'status', value: 'unstarted:todo' }, { field: 'assignee', value: null })))
      .toBe('status=unstarted:todo/assignee=')
  })

  test('drops between project groups are never accepted', () => {
    expect(acceptsDrop([{ field: 'project', value: 'p1' }], [{ field: 'project', value: 'p2' }])).toBe(false)
    expect(acceptsDrop([{ field: 'project', value: 'p1' }, { field: 'priority', value: 'none' }], [{ field: 'project', value: 'p1' }, { field: 'priority', value: 'high' }])).toBe(true)
    expect(acceptsDrop([{ field: 'status', value: 'unstarted:todo' }], [{ field: 'status', value: 'started:doing' }])).toBe(true)
  })

  test('rows are draggable when the order is manual or a group field can change', () => {
    expect(canDrag(display({ group_by: 'project', sub_group_by: 'none', order_by: 'created' }))).toBe(false)
    expect(canDrag(display({ group_by: 'none', order_by: 'created' }))).toBe(false)
    expect(canDrag(display({ group_by: 'project', sub_group_by: 'priority', order_by: 'created' }))).toBe(true)
    expect(canDrag(display({ group_by: 'project', order_by: 'manual' }))).toBe(true)
  })
})

describe('planDrop', () => {
  test('a new priority group becomes a priority patch', () => {
    expect(planDrop(task('a'), [{ field: 'priority', value: 'none' }], [{ field: 'priority', value: 'urgent' }], ctx))
      .toEqual({ kind: 'update', patch: { priority: 'urgent' } })
  })

  test('a drop that changes group and sub-group merges both patches', () => {
    const from = valuesOf({ field: 'status', value: 'unstarted:todo' }, { field: 'priority', value: 'none' })
    const to = valuesOf({ field: 'status', value: 'started:doing' }, { field: 'priority', value: 'urgent' })
    expect(planDrop(task('a'), from, to, ctx)).toEqual({ kind: 'update', patch: { status_id: 'doing', priority: 'urgent' } })
  })

  test('the Duplicate status asks for a canonical task instead of patching', () => {
    expect(planDrop(task('a'), [{ field: 'status', value: 'unstarted:todo' }], [{ field: 'status', value: 'duplicate:duplicate' }], ctx))
      .toEqual({ kind: 'duplicate' })
  })

  test('a status the task\'s project lacks is refused with a message', () => {
    const plan = planDrop(task('a', { projectId: 'p2', statusId: 'todo-2' }), [{ field: 'status', value: 'unstarted:todo' }], [{ field: 'status', value: 'started:doing' }], ctx)
    expect(plan.kind).toBe('error')
    expect(plan.kind === 'error' ? plan.message : '').toContain('No matching status')
  })

  test('a drop into another project group is refused, even alongside other changes', () => {
    const from = valuesOf({ field: 'project', value: 'p1' }, { field: 'priority', value: 'none' })
    const to = valuesOf({ field: 'project', value: 'p2' }, { field: 'priority', value: 'urgent' })
    expect(planDrop(task('a'), from, to, ctx)).toEqual({ kind: 'error', message: 'Tasks can’t be moved between projects.' })
  })

  test('unchanged values plan nothing', () => {
    expect(planDrop(task('a'), [{ field: 'priority', value: 'none' }], [{ field: 'priority', value: 'none' }], ctx)).toEqual({ kind: 'none' })
  })
})

describe('placementUpdates', () => {
  test('a drop into another group writes only the affected integer slots and carries the patch', () => {
    const moving = task('moving', { position: 20, version: 2 })
    const updates = placementUpdates(moving, [task('first', { position: 0, version: 4 }), task('last', { position: 10, version: 6 })], 1, { status_id: 'doing' })
    expect(updates).toEqual([
      { id: 'moving', expected_version: 2, position: 10, status_id: 'doing' },
      { id: 'last', expected_version: 6, position: 11 },
    ])
  })

  test('large groups produce a bounded set of updates', () => {
    const destination = Array.from({ length: 205 }, (_, index) => task(`task-${index}`, { position: index }))
    const updates = placementUpdates(task('moving', { position: 500, version: 2 }), destination, 195, { status_id: 'doing' })
    expect(updates).toHaveLength(11)
    expect(updates[0]).toEqual({ id: 'moving', expected_version: 2, position: 195, status_id: 'doing' })
    expect(updates.at(-1)).toEqual({ id: 'task-204', expected_version: 1, position: 205 })
  })

  test('an in-group move reuses only the changed slots and has no patch', () => {
    const destination = Array.from({ length: 150 }, (_, index) => task(`task-${index}`, { position: index * 10 }))
    const updates = placementUpdates(destination[120]!, destination, 110)
    expect(updates).toHaveLength(11)
    expect(updates.map((update) => update.position)).toEqual(destination.slice(110, 121).map((item) => item.position))
    expect(updates.every((update) => !('status_id' in update))).toBe(true)
  })

  test('dropping a task where it already is writes nothing', () => {
    const destination = [task('a', { position: 0 }), task('b', { position: 1 })]
    expect(placementUpdates(destination[0]!, destination, 0)).toEqual([])
  })
})

test('group values become new-task fields; "no value" groups clear the field', () => {
  expect(groupCreateFields([
    { field: 'status', value: 'started:doing' },
    { field: 'priority', value: 'high' },
  ])).toEqual({ projectId: null, statusKey: 'started:doing', body: { priority: 'high' } })
  expect(groupCreateFields([{ field: 'assignee', value: null }, { field: 'label', value: 'bug' }, { field: 'project', value: 'p2' }]))
    .toEqual({ projectId: 'p2', statusKey: null, body: { assignee_ids: [], label_ids: ['bug'] } })
})

describe('boardGrid', () => {
  test('no grouping falls back to status columns, and empty status columns always show', () => {
    const { columns, lanes } = boardGrid([task('a')], display({ layout: 'board', group_by: 'none' }), ctx)
    expect(columns.map((column) => column.value)).toEqual(expect.arrayContaining(['unstarted:todo', 'started:doing', 'duplicate:duplicate']))
    expect(lanes).toBeNull()
  })

  test('lanes follow the sub-group, and a cell keeps the column order', () => {
    const tasks = [task('late', { priority: 'urgent' }), task('calm'), task('hot', { priority: 'urgent' })]
    const { columns, lanes } = boardGrid(tasks, display({ layout: 'board', sub_group_by: 'priority' }), ctx)
    const todo = columns.find((column) => column.value === 'unstarted:todo')!
    const urgent = lanes!.find((lane) => lane.value === 'urgent')!
    expect(cellTasks(todo, urgent).map((item) => item.id)).toEqual(['late', 'hot'])
    expect(cellTasks(todo, null).map((item) => item.id)).toEqual(['late', 'calm', 'hot'])
  })
})

test('group accents come from the project, label or status colour, else muted', () => {
  const withLabel: GroupContext = { ...ctx, labels: [{ id: 'bug', name: 'Bug', color: '#eb5757', version: 1, workspace_id: 'w' }] }
  expect(groupAccent({ field: 'project', value: 'p2' }, withLabel)).toBe('#26b5ce')
  expect(groupAccent({ field: 'label', value: 'bug' }, withLabel)).toBe('#eb5757')
  expect(groupAccent({ field: 'status', value: 'unstarted:todo' }, withLabel)).toBe('#888')
  expect(groupAccent({ field: 'priority', value: 'urgent' }, withLabel)).toBe('var(--muted-foreground)')
  expect(groupAccent({ field: 'label', value: null }, withLabel)).toBe('var(--muted-foreground)')
})
