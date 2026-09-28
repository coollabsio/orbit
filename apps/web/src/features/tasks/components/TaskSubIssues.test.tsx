import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import type { Project, Task, TaskStatusDef } from '@/features/tasks/api/models'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { SubIssuesSection, type SubIssuesSectionProps } from './TaskSubIssues'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
  window.localStorage.clear()
})

const workspace = { id: 'workspace-1', name: 'Orbit', role: 'owner' as const, version: 1 }
const project = { id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'ORB', color: '#e0457b', created_at: '', updated_at: '', version: 1 } as Project
const todo: TaskStatusDef = { id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#888', category: 'unstarted', position: 0, version: 1 }
const done: TaskStatusDef = { id: 'done', projectId: 'project-1', name: 'Done', description: '', color: '#4cb782', category: 'completed', position: 1, version: 1 }
const parent: Task = {
  id: 'task-0012', identifier: 'ORB-0012', title: 'Checkout redesign', description: '', statusId: 'todo', position: 0, priority: 'high',
  assigneeIds: ['user-1'], projectId: 'project-1', labels: ['label-1'], attachments: [], dueAt: null, createdAt: '', updatedAt: '',
  comments: [], activity: [], version: 1, parentTaskId: null, parent: null, subIssueCount: 0, subIssueClosedCount: 0, ancestors: [],
}
const child = (id: string, title: string, statusId = 'todo') => ({
  id, workspace_id: 'workspace-1', project_id: 'project-1', status_id: statusId, title, description: '', position: 0, priority: 'none',
  assignee_ids: [], creator_id: 'user-1', label_ids: [], created_at: '2026-09-27T10:00:00Z', updated_at: '2026-09-27T10:00:00Z', version: 1,
  duplicate_of: null, blocked: false, parent_task_id: 'task-0012', parent: null, sub_issue_count: 0, sub_issue_closed_count: 0,
})

type Call = { method: string; path: string; search: string; body?: unknown }
function api(calls: Call[], children: Array<ReturnType<typeof child>>) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    const body = request.method === 'GET' ? undefined : await request.json()
    calls.push({ method: request.method, path: url.pathname, search: url.search, body })
    if (url.pathname.endsWith('/projects')) return Response.json({ items: [project], next_cursor: null })
    if (request.method === 'POST' && url.pathname.endsWith('/tasks')) {
      const created = child(`task-01${children.length}0`, (body as { title: string }).title)
      children.push(created)
      return Response.json(created, { status: 201 })
    }
    if (request.method === 'PATCH') return Response.json({ ...child(url.pathname.split('/').at(-1)!, 'x'), parent_task_id: null, version: 2, auto_closed: [] })
    if (url.pathname.endsWith('/tasks')) return Response.json({ items: url.searchParams.get('parent_task_id') === 'task-0012' ? children : [], next_cursor: null })
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
}
const writes = (calls: Call[]) => calls.filter((call) => call.method !== 'GET')

function renderSection(props: Partial<SubIssuesSectionProps> = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider>
    </QueryClientProvider>
  )
  return render(
    <SubIssuesSection parent={parent} projects={[project]} statuses={[todo, done]} users={[]} currentUserId="user-1"
      composing={false} onComposingChange={() => {}} onAddExisting={() => {}} onOpen={() => {}} {...props} />,
    { wrapper },
  )
}

test('Enter creates a sub-issue with the parent\'s defaults and keeps the composer open; Esc closes it', async () => {
  const calls: Call[] = []
  api(calls, [])
  const composing: boolean[] = []
  const view = renderSection({ composing: true, onComposingChange: (open) => composing.push(open) })
  const input = view.getByRole('textbox', { name: 'Sub-issue title' })
  await userEvent.type(input, 'Card validation{Enter}')
  await waitFor(() => expect(writes(calls)).toHaveLength(1))
  expect(writes(calls)[0]!.body).toEqual({
    project_id: 'project-1', status_id: 'todo', priority: 'high', assignee_ids: ['user-1'], parent_task_id: 'task-0012', title: 'Card validation',
  })
  await waitFor(() => expect((input as HTMLInputElement).value).toBe(''))
  expect(document.activeElement).toBe(input)
  expect(await view.findByText('Card validation')).toBeTruthy()
  fireEvent.keyDown(input, { key: 'Escape' })
  expect(composing).toEqual([false])
})

test('Esc in the composer never reaches the task page\'s document-level Esc', () => {
  api([], [])
  const pageEscapes: string[] = []
  const onKeyDown = (event: KeyboardEvent) => { if (event.key === 'Escape') pageEscapes.push('closed task') }
  document.addEventListener('keydown', onKeyDown)
  try {
    const view = renderSection({ composing: true })
    fireEvent.keyDown(view.getByRole('textbox', { name: 'Sub-issue title' }), { key: 'Escape' })
    expect(pageEscapes).toHaveLength(0)
  } finally {
    document.removeEventListener('keydown', onKeyDown)
  }
})

test('without sub-issues and without the composer the section renders nothing', async () => {
  const calls: Call[] = []
  api(calls, [])
  const view = renderSection()
  await waitFor(() => expect(calls.some((call) => call.search.includes('parent_task_id=task-0012'))).toBe(true))
  expect(view.container.querySelectorAll('section')).toHaveLength(0)
})

test('rows list sub-issues in creation order with progress; × removes one from the parent', async () => {
  const calls: Call[] = []
  api(calls, [child('task-0031', 'Payment form', 'done'), child('task-0032', 'Receipt email')])
  const view = renderSection()
  expect(await view.findByRole('img', { name: '1 of 2 sub-issues closed' })).toBeTruthy()
  expect(calls.find((call) => call.search.includes('parent_task_id'))!.search).toContain('sort=created_at')
  fireEvent.click(view.getByRole('button', { name: 'Remove ORB-0031 from parent' }))
  await waitFor(() => expect(writes(calls)).toHaveLength(1))
  expect(writes(calls)[0]).toMatchObject({ method: 'PATCH', path: '/api/v1/workspaces/workspace-1/tasks/task-0031', body: { expected_version: 1, parent_task_id: null } })
})
