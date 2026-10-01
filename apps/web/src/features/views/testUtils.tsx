import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@/test/render'
import type { ReactElement } from 'react'
import { MemoryRouter, useLocation } from 'react-router'
import { queryKeys } from '@/api/queryKeys'
import { ConfirmationModalHost } from '@/components/common/ConfirmationModal'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { SavedView } from './api/views'
import { member } from './testFixtures'
import { DEFAULT_DISPLAY } from './viewState'
import type { WorkspaceRole } from '@/api/generated/types.gen'
import { testWorkspace } from '@/test/workspace'

function LocationProbe() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

export function savedView(overrides: Partial<SavedView> & Pick<SavedView, 'id' | 'name'>): SavedView {
  return {
    workspace_id: 'alpha',
    owner: { user_id: 'user-1', display_name: 'Ada' },
    description: '',
    icon: null,
    color: null,
    visibility: 'personal',
    state: { filter: { op: 'and', children: [] }, display: DEFAULT_DISPLAY },
    state_error: null,
    version: 1,
    is_favorite: false,
    favorite_position: null,
    can_edit: true,
    can_change_visibility: true,
    created_at: '2026-09-01T12:00:00Z',
    updated_at: '2026-09-01T12:00:00Z',
    ...overrides,
  } as SavedView
}

type RecordedRequest = { method: string; path: string; body: unknown }

/** Replaces global fetch; restore it in `afterEach`. Records method, path, and JSON body. */
export function stubFetch(handler: (request: Request) => Response | Promise<Response>): RecordedRequest[] {
  const requests: RecordedRequest[] = []
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const body = request.method === 'GET' || request.method === 'DELETE' ? undefined : await request.clone().json().catch(() => undefined)
    requests.push({ method: request.method, path: new URL(request.url).pathname, body })
    return handler(request)
  }) as typeof fetch
  return requests
}

export function renderWithProviders(ui: ReactElement, { route = '/', views = [], role = 'member' }: { route?: string; views?: SavedView[]; role?: WorkspaceRole } = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  const workspace = testWorkspace(role, { id: 'alpha', name: 'Alpha' })
  client.setQueryData(queryKeys.currentUser, { id: 'user-1', display_name: 'Ada', email: 'ada@orbit.test', installation_admin: false })
  client.setQueryData(queryKeys.views(workspace.id), views)
  client.setQueryData(queryKeys.members(workspace.id), [member('user-1', 'Ada'), member('user-2', 'Grace')])
  const view = render(
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
        <MemoryRouter initialEntries={[route]}>
          <ConfirmationModalHost />
          {ui}
          <LocationProbe />
        </MemoryRouter>
      </WorkspaceContext.Provider>
    </QueryClientProvider>,
  )
  return { view, client }
}
