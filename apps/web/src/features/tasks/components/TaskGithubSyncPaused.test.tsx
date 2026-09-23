import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { Task, TaskViewState } from '@/features/tasks/api/models'
import { TaskDetail } from './TaskDetail'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const workspace = { id: 'workspace-1', name: 'Orbit', role: 'owner' as const, version: 1 }
const task: Task = {
  id: 'task-1', identifier: 'ORB-1', title: 'GitHub task', description: '', sourceUrl: 'https://github.com/acme/repo/issues/12', statusId: 'todo',
  position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
  labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
}
const state: TaskViewState = { currentUserId: 'user-1', users: [], statuses: [], labels: [], tasks: [task] }

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return <QueryClientProvider client={client}><MemoryRouter><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider></MemoryRouter></QueryClientProvider>
}

test('task shows a paused badge when its GitHub project label is removed', async () => {
  globalThis.fetch = (async () => Response.json([{ kind: 'issue', title: 'acme/repo#12', url: 'https://github.com/acme/repo/issues/12', state: 'paused', source: true }])) as unknown as typeof fetch
  const view = render(<TaskDetail task={task} project={undefined} state={state} onBack={() => {}} />, { wrapper: Wrapper })
  expect(await view.findByText('GitHub sync paused')).toBeTruthy()
  expect(view.getByText('Add the project label to the GitHub issue or pull request again to resume updates.')).toBeTruthy()
  expect(view.getByText("This task's title and description are read-only while it is linked to GitHub.")).toBeTruthy()
})

test('task has no paused badge while GitHub sync is active', async () => {
  globalThis.fetch = (async () => Response.json([{ kind: 'issue', title: 'acme/repo#12', url: 'https://github.com/acme/repo/issues/12', state: 'active', source: true }])) as unknown as typeof fetch
  const view = render(<TaskDetail task={task} project={undefined} state={state} onBack={() => {}} />, { wrapper: Wrapper })
  await view.findByText("This task's title and description are read-only while it is linked to GitHub.")
  expect(view.queryByRole('region', { name: 'GitHub links' })).toBeNull()
  expect(view.getByRole('link', { name: 'github.com' })).toHaveProperty('href', 'https://github.com/acme/repo/issues/12')
  expect(view.queryByText('GitHub sync paused')).toBeNull()
})


test('related pull requests stay in GitHub without a duplicate issue row', async () => {
  globalThis.fetch = (async () => Response.json([
    { kind: 'issue', title: 'acme/repo#12', url: 'https://github.com/acme/repo/issues/12', state: 'active', source: true },
    { kind: 'pull_request', title: 'Fix task', url: 'https://github.com/acme/repo/pull/20', state: 'open', source: false },
  ])) as unknown as typeof fetch
  const view = render(<TaskDetail task={task} project={undefined} state={state} onBack={() => {}} />, { wrapper: Wrapper })
  const region = await view.findByRole('region', { name: 'GitHub links' })
  expect(region.querySelectorAll('a')).toHaveLength(1)
  expect(region.textContent).toContain('Fix task')
  expect(region.textContent).not.toContain('acme/repo#12')
  expect(view.getByRole('link', { name: 'github.com' })).toHaveProperty('href', 'https://github.com/acme/repo/issues/12')
})

test('pull request tasks use the source link without a duplicate GitHub section', async () => {
  globalThis.fetch = (async () => Response.json([{ kind: 'pull_request', title: 'acme/repo#8', url: 'https://github.com/acme/repo/pull/8', state: 'paused', source: true }])) as unknown as typeof fetch
  const view = render(<TaskDetail task={{ ...task, sourceUrl: 'https://github.com/acme/repo/pull/8' }} project={undefined} state={state} onBack={() => {}} />, { wrapper: Wrapper })
  expect(await view.findByText('GitHub sync paused')).toBeTruthy()
  expect(view.getByText("This task's title and description are read-only while it is linked to GitHub.")).toBeTruthy()
  expect(view.queryByRole('region', { name: 'GitHub links' })).toBeNull()
  expect(view.getByRole('link', { name: 'github.com' })).toHaveProperty('href', 'https://github.com/acme/repo/pull/8')
})
