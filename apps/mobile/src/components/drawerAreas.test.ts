import { describe, expect, test } from 'bun:test'

import { drawerAreaOf, drawerSections, tasksHref } from './drawerAreas'

// the shape of Shell.tsx's WORKSPACE_NAV, ACCOUNT_NAV and ADMIN_NAV (Shell.tsx itself loads react-native)
const areas = {
  workspace: [{ href: '/settings' }, { href: '/settings/file-storage' }, { href: '/settings/members' }],
  account: [{ href: '/settings/profile' }, { href: '/settings/appearance' }, { href: '/settings/notifications' }],
  admin: [{ href: '/settings/admin' }, { href: '/settings/admin/users' }, { href: '/settings/admin/audit' }, { href: '/settings/admin/storage' }],
}

describe('drawerAreaOf', () => {
  test('maps settings routes to their area', () => {
    expect(drawerAreaOf('/settings', areas)).toBe('workspace')
    expect(drawerAreaOf('/settings/members', areas)).toBe('workspace')
    expect(drawerAreaOf('/settings/profile', areas)).toBe('account')
    expect(drawerAreaOf('/settings/appearance', areas)).toBe('account')
    expect(drawerAreaOf('/settings/admin', areas)).toBe('admin')
    expect(drawerAreaOf('/settings/admin/jobs', areas)).toBe('admin')
    expect(drawerAreaOf('/settings/admin/users', areas)).toBe('admin')
    expect(drawerAreaOf('/settings/admin/storage', areas)).toBe('admin')
  })

  test('an unlisted page under /settings is the workspace settings', () => {
    expect(drawerAreaOf('/settings/unknown', areas)).toBe('workspace')
  })

  test('task routes are the Tasks area', () => {
    expect(drawerAreaOf('/tasks', areas)).toBe('tasks')
    expect(drawerAreaOf('/tasks/abc', areas)).toBe('tasks')
    expect(drawerAreaOf('/tasks-trash', areas)).toBeNull()
  })

  test('app routes have no area', () => {
    expect(drawerAreaOf('/', areas)).toBeNull()
    expect(drawerAreaOf('/docs', areas)).toBeNull()
    expect(drawerAreaOf('/settingsx', areas)).toBeNull()
  })
})

describe('drawerSections', () => {
  test('groups links under the web section labels', () => {
    expect(drawerSections('workspace', areas.workspace)).toEqual([
      { label: 'Workspace', items: [{ href: '/settings' }, { href: '/settings/members' }] },
      { label: 'Integrations', items: [{ href: '/settings/file-storage' }] },
    ])
    expect(drawerSections('admin', areas.admin)).toEqual([
      { label: 'Instance', items: [{ href: '/settings/admin' }, { href: '/settings/admin/users' }] },
      { label: 'Data', items: [{ href: '/settings/admin/storage' }] },
      { label: 'Monitoring', items: [{ href: '/settings/admin/audit' }] },
    ])
    expect(drawerSections('account', areas.account)).toEqual([
      { label: 'Account', items: [{ href: '/settings/profile' }] },
      { label: 'Preferences', items: [{ href: '/settings/appearance' }, { href: '/settings/notifications' }] },
    ])
  })
})

describe('tasksHref', () => {
  test('a preset keeps the project, a project keeps the preset', () => {
    expect(tasksHref({ view: 'mine', project: 'p1' }, { view: 'overdue' })).toBe('/tasks?project=p1&view=overdue')
    expect(tasksHref({ view: 'mine' }, { project: 'p2' })).toBe('/tasks?project=p2&view=mine')
  })

  test('null clears a key', () => {
    expect(tasksHref({ view: 'mine', project: 'p1' }, { view: null })).toBe('/tasks?project=p1')
    expect(tasksHref({ view: 'mine', project: 'p1' }, { project: null })).toBe('/tasks?view=mine')
    expect(tasksHref({}, { view: null })).toBe('/tasks')
  })
})
