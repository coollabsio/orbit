import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { render } from '@/test/render'
import { NewTaskProvider, useCreateTaskDefaults } from './newTask'
import { testWorkspace } from '@/test/workspace'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const workspace = testWorkspace()
const project = (id: string, name: string) => ({ id, workspace_id: 'workspace-1', name, key: name.slice(0, 3).toUpperCase(), color: '#e0457b', created_at: '', updated_at: '', version: 1, auto_close_parent: true, auto_close_sub_issues: true })

function api() {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = new URL((input as Request).url)
    if (url.pathname.endsWith('/projects')) return Response.json({ items: [project('project-1', 'Launch'), project('project-2', 'Docs')], next_cursor: null })
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
}

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return <QueryClientProvider client={client}><MemoryRouter><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}><NewTaskProvider>{children}</NewTaskProvider></WorkspaceContext.Provider></MemoryRouter></QueryClientProvider>
}

function Page() {
  useCreateTaskDefaults({ defaults: () => ({ project_id: 'project-2' }) })
  return null
}

test('C opens the new task dialog on a page without defaults', async () => {
  api()
  const view = render(<input aria-label="Other field" />, { wrapper: Wrapper })
  await userEvent.keyboard('c')
  await view.findByLabelText('Task title')
  // the first project
  await view.findByRole('button', { name: 'Project: Launch' })
})

test('C uses the defaults of the page', async () => {
  api()
  const view = render(<Page />, { wrapper: Wrapper })
  await userEvent.keyboard('c')
  await view.findByRole('button', { name: 'Project: Docs' })
})

test('C does nothing while typing, and never opens a second dialog', async () => {
  api()
  const view = render(<input aria-label="Other field" />, { wrapper: Wrapper })
  await userEvent.type(view.getByLabelText('Other field'), 'c')
  expect(view.queryByLabelText('Task title')).toBeNull()
  view.getByLabelText('Other field').blur()
  await userEvent.keyboard('c')
  await view.findByLabelText('Task title')
  ;(document.activeElement as HTMLElement).blur()
  await userEvent.keyboard('c')
  expect(view.getAllByLabelText('Task title')).toHaveLength(1)
})
