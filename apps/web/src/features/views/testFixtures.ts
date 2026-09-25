import type { LabelRecord, ProjectRecord } from '@/api/generated/types.gen'
import type { TaskStatusDef } from '@/features/tasks/api/models'
import type { User } from '@/features/workspaces/models'
import type { FilterOptions } from './filterFields'

export function member(id: string, name: string): User {
  return {
    id,
    membershipId: `membership-${id}`,
    name,
    handle: name.toLowerCase(),
    email: `${name.toLowerCase()}@orbit.test`,
    role: 'Member',
    color: '#e0457b',
    online: false,
    title: '',
    roleIds: [],
    version: 1,
  }
}

function status(id: string, projectId: string, name: string, category: TaskStatusDef['category'], position: number): TaskStatusDef {
  return { id, projectId, name, description: '', color: '#8b8f98', category, position, version: 1 }
}

function label(id: string, name: string, color: string): LabelRecord {
  return { id, name, color, version: 1, workspace_id: 'alpha' }
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
}

export const FILTER_OPTIONS: FilterOptions = {
  statuses: [
    status('todo-a', 'project-a', 'Todo', 'unstarted', 0),
    status('todo-b', 'project-b', 'Todo', 'unstarted', 0),
    status('done-a', 'project-a', 'Done', 'completed', 1),
  ],
  members: [member('user-1', 'Ada'), member('user-2', 'Grace')],
  labels: [label('label-bug', 'Bug', '#eb5757'), label('label-ui', 'UI', '#26b5ce'), label('label-docs', 'Docs', '#4cb782')],
  projects: [LAUNCH],
  currentUserId: 'user-1',
}
