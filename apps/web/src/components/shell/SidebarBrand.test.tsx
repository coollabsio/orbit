import { expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { WorkspaceRecord } from '../../api/generated/types.gen'
import { WorkspaceContext } from '../../features/workspaces/workspaceContext'
import { SidebarBrand } from './SidebarBrand'

const workspace: WorkspaceRecord = { id: 'workspace-1', name: 'Orbit', role: 'owner', version: 1 }

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return (
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
          {children}
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>
  )
}

test('the brand row carries the workspace switcher and no collapse control', () => {
  const view = render(<SidebarBrand />, { wrapper: Wrapper })

  const brand = view.container.querySelector('.app-sidebar-brand') as HTMLElement
  expect(brand).not.toBeNull()
  expect(brand.querySelector('.workspace-switcher')).not.toBeNull()
  expect(brand.querySelector('.app-sidebar-collapse')).toBeNull()
  expect(view.queryByRole('button', { name: 'Collapse sidebar' })).toBeNull()
})

test('the mobile drawer brand row has no collapse control either', () => {
  const view = render(<SidebarBrand onSelectWorkspace={() => {}} />, { wrapper: Wrapper })

  expect(view.container.querySelector('.app-sidebar-collapse')).toBeNull()
  expect(view.queryByRole('button', { name: 'Collapse sidebar' })).toBeNull()
})
