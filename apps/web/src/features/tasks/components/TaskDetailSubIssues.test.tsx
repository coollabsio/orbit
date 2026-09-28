import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { Task, TaskStatusDef, TaskViewState } from '@/features/tasks/api/models'
import { TaskDetail } from './TaskDetail'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
  window.localStorage.clear()
})

const workspace = { id: 'workspace-1', name: 'Orbit', role: 'owner' as const, version: 1 }
const project = { id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'ORB', color: '#e0457b', created_at: '', updated_at: '', version: 1, auto_close_parent: true, auto_close_sub_issues: true }
const todo: TaskStatusDef = { id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#888', category: 'unstarted', position: 0, version: 1 }
const state: TaskViewState = { currentUserId: 'user-1', users: [], statuses: [todo], labels: [], tasks: [] }
const task: Task = {
  id: 'task-0012', identifier: 'ORB-0012', title: 'Checkout redesign', description: '', statusId: 'todo', position: 0, priority: 'none',
  assigneeIds: [], creatorId: 'user-1', projectId: 'project-1', labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '',
  comments: [], activity: [], version: 1, duplicateOf: null, blocked: false, parentTaskId: null, parent: null, subIssueCount: 0, subIssueClosedCount: 0, ancestors: [],
}
const record = (id: string, title: string, parentId: string | null = null, subIssueCount = 0) => ({
  id, workspace_id: 'workspace-1', project_id: 'project-1', status_id: 'todo', title, description: '', position: 0, priority: 'none',
  assignee_ids: [], creator_id: 'user-1', label_ids: [], created_at: '', updated_at: '', version: 1, duplicate_of: null, blocked: false,
  parent_task_id: parentId, parent: null, sub_issue_count: subIssueCount, sub_issue_closed_count: 0,
})

type Call = { method: string; path: string; body?: unknown }
/** `tree`: parent id → its direct sub-issues; `all`: what the picker lists. */
function api(calls: Call[], tree: Record<string, unknown[]>, all: unknown[]) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    const body = request.method === 'PATCH' || request.method === 'POST' ? await request.json() : undefined
    calls.push({ method: request.method, path: url.pathname, body })
    if (url.pathname.endsWith('/github-links') || url.pathname.endsWith('/relations')) return Response.json([])
    if (url.pathname.endsWith('/projects')) return Response.json({ items: [project], next_cursor: null })
    if (request.method === 'PATCH') return Response.json({ ...record(url.pathname.split('/').at(-1)!, 'x'), version: 2, auto_closed: [] })
    if (url.pathname.endsWith('/tasks')) {
      const parentId = url.searchParams.get('parent_task_id')
      return Response.json({ items: parentId ? tree[parentId] ?? [] : all, next_cursor: null })
    }
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
}
const writes = (calls: Call[]) => calls.filter((call) => call.method !== 'GET')

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  return <QueryClientProvider client={client}><MemoryRouter><WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider></MemoryRouter></QueryClientProvider>
}

test('a sub-issue shows its ancestors in the header and its parent as a property that can be removed', async () => {
  const calls: Call[] = []
  api(calls, {}, [])
  const opened: string[] = []
  const view = render(
    <TaskDetail
      task={{ ...task, id: 'task-0031', identifier: 'ORB-0031', parentTaskId: 'task-0012', parent: { id: 'task-0012', title: 'Checkout redesign', projectKey: 'ORB' }, ancestors: [{ id: 'task-0012', title: 'Checkout redesign', projectKey: 'ORB' }] }}
      project={project} state={state} onBack={() => {}} onOpenTask={(id) => opened.push(id)}
    />,
    { wrapper: Wrapper },
  )
  expect(view.getByRole('navigation', { name: 'breadcrumb' }).textContent).toBe('ORB-0012Checkout redesignORB-0031')
  expect(view.queryAllByRole('button', { name: 'Set parent' })).toHaveLength(0)
  fireEvent.click(view.getByRole('button', { name: 'Parent: ORB-0012 Checkout redesign' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Remove parent' }))
  await waitFor(() => expect(writes(calls)).toHaveLength(1))
  expect(writes(calls)[0]).toEqual({ method: 'PATCH', path: '/api/v1/workspaces/workspace-1/tasks/task-0031', body: { expected_version: 1, parent_task_id: null } })
})

test('"Set parent" never offers the task or anything loaded below it', async () => {
  const calls: Call[] = []
  api(calls, {
    'task-0012': [record('task-0031', 'Payment form', 'task-0012', 1)],
    'task-0031': [record('task-0045', 'Card validation', 'task-0031')],
  }, [record('task-0012', 'Checkout redesign'), record('task-0031', 'Payment form', 'task-0012', 1), record('task-0045', 'Card validation', 'task-0031'), record('task-0099', 'Pricing page')])
  const view = render(<TaskDetail task={{ ...task, subIssueCount: 1 }} project={project} state={state} onBack={() => {}} />, { wrapper: Wrapper })
  // the nested level is loaded: the grandchild shows in the Sub-issues section
  const section = await view.findByRole('region', { name: 'Sub-issues' })
  await within(section).findByText('Card validation')
  // with sub-issues the section's own + replaces the "Add sub-issue" button
  expect(view.queryAllByRole('button', { name: /^Add sub-issue$/ }).filter((button) => !section.contains(button))).toHaveLength(0)
  fireEvent.click(view.getByRole('button', { name: 'Set parent' }))
  const picker = await view.findByRole('dialog', { name: 'Set parent of ORB-0012…' })
  await within(picker).findByRole('option', { name: /Pricing page/ })
  expect(within(picker).queryAllByRole('option')).toHaveLength(1)
}, 20000)
