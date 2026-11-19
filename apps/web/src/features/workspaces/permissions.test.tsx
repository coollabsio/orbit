import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { WorkspaceRecord, WorkspaceRole } from '@orbit/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { testWorkspace } from '@/test/workspace'
import { canPurgeTask, useCan, useCanIn, useIsInstallationAdmin } from './permissions'
import { WorkspaceContext } from './workspaceContext'

function wrapper(role: WorkspaceRole, installationAdmin: boolean, overrides: Partial<WorkspaceRecord> = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.currentUser, { id: 'user-1', email: 'a@orbit.test', display_name: 'A', installation_admin: installationAdmin })
  const workspace = testWorkspace(role, overrides)
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider>
    </QueryClientProvider>
  )
}

test('permissions come from the workspace record, not from the role name', () => {
  const can = (role: WorkspaceRole) => renderHook(() => [useCan('members.manage'), useCan('workspace.delete')], { wrapper: wrapper(role, false) }).result.current
  expect(can('owner')).toEqual([true, true])
  expect(can('admin')).toEqual([true, false])
  expect(can('member')).toEqual([false, false])
})

test('installation admin is read from the cached current user', () => {
  expect(renderHook(() => useIsInstallationAdmin(), { wrapper: wrapper('member', true) }).result.current).toBeTrue()
  expect(renderHook(() => useIsInstallationAdmin(), { wrapper: wrapper('owner', false) }).result.current).toBeFalse()
})

test('a project grant covers only its project; a workspace-wide permission covers every project', () => {
  const grant = { permissions: [], project_permissions: [{ project_id: 'project-a', permissions: ['tasks.purge' as const] }] }
  const canIn = (projectId: string | undefined, overrides: Partial<WorkspaceRecord>) =>
    renderHook(() => useCanIn('tasks.purge', projectId), { wrapper: wrapper('member', false, overrides) }).result.current
  expect(canIn('project-a', grant)).toBeTrue()
  expect(canIn('project-b', grant)).toBeFalse()
  expect(canIn(undefined, grant)).toBeFalse()
  expect(canIn('project-b', { permissions: ['tasks.purge'], project_permissions: [] })).toBeTrue()
  expect(canIn('project-a', { permissions: [], project_permissions: [] })).toBeFalse()
})

test('purging a task needs pages.purge, or tasks.purge in the project of the task', () => {
  expect(canPurgeTask(testWorkspace('member', { permissions: ['pages.purge'] }), 'project-b')).toBeTrue()
  const granted = testWorkspace('member', { permissions: [], project_permissions: [{ project_id: 'project-a', permissions: ['tasks.purge'] }] })
  expect(canPurgeTask(granted, 'project-a')).toBeTrue()
  expect(canPurgeTask(granted, 'project-b')).toBeFalse()
  const otherGrant = testWorkspace('member', { permissions: [], project_permissions: [{ project_id: 'project-a', permissions: ['comments.moderate'] }] })
  expect(canPurgeTask(otherGrant, 'project-a')).toBeFalse()
})
