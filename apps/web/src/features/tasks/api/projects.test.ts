import { expect, test } from 'bun:test'
import type { StatusRecord } from '@/api/generated/types.gen'
import { statusFromRecord } from './projects'

test('generated status records preserve project, category, order, and version', () => {
  const status: StatusRecord = {
    id: 'status-1', workspace_id: 'workspace-1', project_id: 'project-1', name: 'In progress',
    description: 'Under way', color: '#abcdef', category: 'started', position: 2, version: 5,
  }
  expect(statusFromRecord(status)).toEqual({
    id: 'status-1', projectId: 'project-1', name: 'In progress', description: 'Under way',
    color: '#abcdef', category: 'started', position: 2, version: 5,
  })
})

test('the duplicate category survives the status mapping', () => {
  const status: StatusRecord = {
    id: 'status-dup', workspace_id: 'workspace-1', project_id: 'project-1', name: 'Duplicate',
    description: '', color: '#8b8f98', category: 'duplicate', position: 4, version: 1,
  }
  expect(statusFromRecord(status).category).toBe('duplicate')
})
