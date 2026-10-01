import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { WorkspaceRole } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { testWorkspace } from '@/test/workspace'
import { useCan, useIsInstallationAdmin } from './permissions'
import { WorkspaceContext } from './workspaceContext'

function wrapper(role: WorkspaceRole, installationAdmin: boolean) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.currentUser, { id: 'user-1', email: 'a@orbit.test', display_name: 'A', installation_admin: installationAdmin })
  const workspace = testWorkspace(role)
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
