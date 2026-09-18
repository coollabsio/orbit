import { expect, mock, test } from 'bun:test'
import { fireEvent, render, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import type { WorkspaceRecord } from '../../api/generated/types.gen'
import { WorkspaceContext } from '../../features/workspaces/workspaceContext'
import { Topbar } from './Topbar'
import { TopbarBreadcrumb } from './TopbarBreadcrumb'
import { TopbarSlot, TopbarSlotProvider } from './TopbarSlot'

const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }

function renderTopbar(onOpenDrawer = () => {}) {
  return render(
    <MemoryRouter>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
        <TopbarSlotProvider>
          <Topbar onOpenDrawer={onOpenDrawer} />
          <TopbarSlot side="left"><TopbarBreadcrumb crumbs={[{ label: 'ORB-12' }]} /></TopbarSlot>
          <TopbarSlot side="right"><button type="button">New task</button></TopbarSlot>
        </TopbarSlotProvider>
      </WorkspaceContext.Provider>
    </MemoryRouter>,
  )
}

test('the topbar shows the workspace root crumb and hosts page-supplied chrome', () => {
  const view = renderTopbar()
  const bar = view.container.querySelector('.topbar') as HTMLElement

  expect(within(bar).getByRole('link', { name: 'Orbit' }).getAttribute('href')).toBe('/')
  const left = bar.querySelector('[data-slot="left"]') as HTMLElement
  const right = bar.querySelector('[data-slot="right"]') as HTMLElement
  expect(within(left).getByText('ORB-12')).not.toBeNull()
  expect(within(right).getByRole('button', { name: 'New task' })).not.toBeNull()
})

test('the drawer button opens the mobile sidebar directly', () => {
  const onOpenDrawer = mock(() => {})
  const view = renderTopbar(onOpenDrawer)

  fireEvent.click(view.getByRole('button', { name: 'Menu' }))
  expect(onOpenDrawer).toHaveBeenCalledTimes(1)
})

test('the topbar no longer loads project, status or task data on every route', async () => {
  const source = await Bun.file(new URL('./Topbar.tsx', import.meta.url)).text()

  expect(source).not.toContain('crumbsFor')
  expect(source).not.toContain('useProjects')
  expect(source).not.toContain('useAllStatuses')
  expect(source).not.toContain('useTasks')
  expect(source).not.toContain('useAppState')
  expect(source).not.toContain('data-root')
})
