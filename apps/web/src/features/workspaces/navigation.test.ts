import { expect, test } from 'bun:test'
import type { WorkspaceRecord } from '@/api/generated/types.gen'
import { selectedWorkspaceId, switchWorkspaceHref } from './navigation'

const workspaces: WorkspaceRecord[] = [
  { id: 'alpha', name: 'Alpha', role: 'owner', version: 1 },
  { id: 'beta', name: 'Beta', role: 'member', version: 3 },
]

test('route selection wins over the saved navigation preference', () => {
  expect(selectedWorkspaceId(workspaces, '?workspace=beta', 'alpha')).toBe('beta')
})

test('an unavailable selection cannot become authorization state', () => {
  expect(selectedWorkspaceId(workspaces, '?workspace=deleted', 'also-deleted')).toBe('alpha')
  expect(selectedWorkspaceId([], '?workspace=alpha', 'alpha')).toBeNull()
})

test('workspace switching keeps the current route and unrelated filters', () => {
  expect(switchWorkspaceHref('/tasks/task-1', '?project=p1&workspace=alpha', 'beta')).toBe(
    '/tasks/task-1?project=p1&workspace=beta',
  )
})
