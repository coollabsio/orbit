import type { LabelRecord, ProjectRecord, TaskField, TaskFieldKind, TaskFieldOption } from '@orbit/api/generated/types.gen'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import type { FilterOptions } from './filterFields'
import { NO_MEMBER_ABILITIES } from '@/features/workspaces/models'

export function member(id: string, name: string): User {
  return {
    id,
    membershipId: `membership-${id}`,
    name,
    handle: name.toLowerCase(),
    email: `${name.toLowerCase()}@orbit.test`,
    role: 'Member',
    color: '#e0457b',
    title: '',
    roleIds: [], can: NO_MEMBER_ABILITIES,
    version: 1,
  }
}

function status(id: string, projectId: string, name: string, category: TaskStatusDef['category'], position: number): TaskStatusDef {
  return { id, projectId, name, description: '', color: '#8b8f98', category, position, version: 1 }
}

function label(id: string, name: string, color: string): LabelRecord {
  return { id, name, color, version: 1, workspace_id: 'alpha', project_id: 'project-a' }
}

const LAUNCH: ProjectRecord = {
  id: 'project-a',
  name: 'Launch',
  key: 'LAU',
  color: '#e0457b',
  workspace_id: 'alpha',
  version: 1,
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
  auto_close_parent: true,
  auto_close_sub_issues: true,
  private: false,
}

export function taskField(id: string, name: string, kind: TaskFieldKind, options: TaskFieldOption[] = []): TaskField {
  return {
    id, name, kind, options, can_manage: true, created_at: '2026-11-01T00:00:00Z', created_by: 'user-1', project_count: 1,
    updated_at: '2026-11-01T00:00:00Z', version: 1, workspace_id: 'alpha',
  }
}

/** One custom field per kind. */
export const TASK_FIELDS: TaskField[] = [
  taskField('field-customer', 'Customer', 'select', [{ id: 'opt-acme', name: 'Acme', color: '#eb5757' }, { id: 'opt-globex', name: 'Globex', color: '#26b5ce' }]),
  taskField('field-budget', 'Budget', 'number'),
  taskField('field-signed', 'Signed off', 'checkbox'),
  taskField('field-launch', 'Launch day', 'date'),
  taskField('field-tags', 'Areas', 'multi_select', [{ id: 'opt-web', name: 'Web', color: '#4cb782' }]),
  taskField('field-notes', 'Notes', 'text'),
]

export const FILTER_OPTIONS: FilterOptions = {
  statuses: [
    status('todo-a', 'project-a', 'Todo', 'unstarted', 0),
    status('todo-b', 'project-b', 'Todo', 'unstarted', 0),
    status('done-a', 'project-a', 'Done', 'completed', 1),
  ],
  members: [member('user-1', 'Ada'), member('user-2', 'Grace')],
  labels: [label('label-bug', 'Bug', '#eb5757'), label('label-ui', 'UI', '#26b5ce'), label('label-docs', 'Docs', '#4cb782')],
  projects: [LAUNCH],
  milestones: [],
  cycles: [],
  currentUserId: 'user-1',
  taskFields: TASK_FIELDS,
}
