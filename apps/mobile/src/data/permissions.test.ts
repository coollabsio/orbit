import { expect, test } from 'bun:test'
import { canInProject, canManageProject, canPurgeTask } from './permissions'

test('a workspace-wide permission covers every project', () => {
  const workspace = { permissions: ['tasks.purge' as const], project_permissions: [] }
  expect(canInProject(workspace, 'tasks.purge', 'p1')).toBe(true)
  expect(canInProject(workspace, 'tasks.purge', 'p2')).toBe(true)
  expect(canPurgeTask(workspace, 'p2')).toBe(true)
})

test('a project grant covers only that project', () => {
  const workspace = { permissions: [], project_permissions: [{ project_id: 'p1', permissions: ['tasks.purge' as const] }] }
  expect(canInProject(workspace, 'tasks.purge', 'p1')).toBe(true)
  expect(canInProject(workspace, 'tasks.purge', 'p2')).toBe(false)
  expect(canInProject(workspace, 'comments.moderate', 'p1')).toBe(false)
  expect(canPurgeTask(workspace, 'p1')).toBe(true)
  expect(canPurgeTask(workspace, 'p2')).toBe(false)
})

test('pages.purge purges tasks in every project', () => {
  expect(canPurgeTask({ permissions: ['pages.purge'], project_permissions: [] }, 'p1')).toBe(true)
})

test('no grant, or no project_permissions field, purges nothing', () => {
  expect(canPurgeTask({ permissions: ['tasks.create'], project_permissions: [] }, 'p1')).toBe(false)
  expect(canPurgeTask({ permissions: [] }, 'p1')).toBe(false)
})

test('projects.manage granted in one project manages only that project', () => {
  const workspace = { permissions: [], project_permissions: [{ project_id: 'p1', permissions: ['projects.manage' as const] }] }
  expect(canInProject(workspace, 'projects.manage', 'p1')).toBe(true)
  expect(canInProject(workspace, 'projects.manage', 'p2')).toBe(false)
  expect(canInProject({ permissions: ['projects.manage'], project_permissions: [] }, 'projects.manage', 'p2')).toBe(true)
})

test('a project is managed with projects.manage in it, or by its creator', () => {
  const none = { permissions: [], project_permissions: [] }
  const granted = { permissions: [], project_permissions: [{ project_id: 'p1', permissions: ['projects.manage' as const] }] }
  expect(canManageProject(granted, 'me', { id: 'p1', created_by: 'ada' })).toBe(true)
  expect(canManageProject(granted, 'me', { id: 'p2', created_by: 'ada' })).toBe(false)
  expect(canManageProject(none, 'me', { id: 'p2', created_by: 'me' })).toBe(true)
  expect(canManageProject(none, 'me', { id: 'p2', created_by: null })).toBe(false)
  expect(canManageProject(none, 'me', { id: 'p2' })).toBe(false)
  expect(canManageProject(none, '', { id: 'p2', created_by: '' })).toBe(false)
  expect(canManageProject(none, undefined, { id: 'p2' })).toBe(false)
})
