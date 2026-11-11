import type { ReactNode } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { Toaster } from 'sonner'
import type { WorkspaceRole } from '@orbit/api/generated/types.gen'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { testWorkspace } from '@/test/workspace'

/** Renders `ui` inside a query client, a router at `path`, the test workspace with `role`, and a toaster. */
export function renderWithWorkspace(ui: ReactNode, { role = 'owner', path = '/' }: { role?: WorkspaceRole; path?: string } = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const workspace = testWorkspace(role)
  return render(<QueryClientProvider client={client}>
    <MemoryRouter initialEntries={[path]}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{ui}<Toaster /></WorkspaceContext.Provider>
    </MemoryRouter>
  </QueryClientProvider>)
}
