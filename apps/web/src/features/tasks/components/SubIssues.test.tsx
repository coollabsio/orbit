import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { ProjectRecord, TaskRecord, WorkspaceRecord } from '../../../api/generated/types.gen'
import { WorkspaceContext } from '../../workspaces/workspaceContext'
import type { Task, TaskViewState } from '../api/models'
import { SubIssues } from './SubIssues'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

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

const parent: Task = {
  id: 'task-1', identifier: 'ORB-1', title: 'Parent', descriptionJson: { type: 'doc', content: [] }, descriptionText: '',
  statusId: 'todo', position: 0, priority: 'none', assigneeIds: [], creatorId: 'user-1', projectId: 'project-1',
  labels: [], attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
  parentId: null, subIssueTotal: 5, subIssueDone: 3, duplicateOfTaskId: null, duplicateIds: [], referencedBy: [],
}

const state: TaskViewState = {
  currentUserId: 'user-1',
  users: [],
  labels: [],
  tasks: [parent],
  statuses: [{ id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#8b8f98', category: 'unstarted', position: 0, version: 0 }],
}

function child(id: string, identifier: string, projectId: string): TaskRecord {
  return {
    id, workspace_id: 'workspace-1', project_id: projectId, status_id: 'todo', identifier, identifier_key: 'ORB', number: 2,
    title: `Child ${identifier}`, description_json: { type: 'doc', content: [] }, description_text: '', position: 0,
    priority: 'none', assignee_ids: [], creator_id: 'user-1', label_ids: [], parent_id: 'task-1',
    sub_issue_total: 0, sub_issue_done: 0, duplicate_ids: [], referenced_by: [],
    created_at: '', updated_at: '', version: 0,
  }
}

const otherProject: ProjectRecord = {
  id: 'project-2', workspace_id: 'workspace-1', name: 'Design', key: 'DES', color: '#e5484d',
  created_at: '', updated_at: '', version: 0,
}

test('sub-issues header shows the done/total count and a progress bar', async () => {
  const urls: string[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    urls.push((input as Request).url)
    return Response.json({ items: [child('task-2', 'ORB-2', 'project-1'), child('task-3', 'DES-1', 'project-2')], next_cursor: null })
  }) as unknown as typeof fetch
  const view = render(<SubIssues task={parent} projects={[otherProject]} state={state} onOpen={() => {}} />, { wrapper: Wrapper })

  expect(view.getByText('Sub-issues')).toBeTruthy()
  expect(view.getByText('3/5')).toBeTruthy()
  const bar = view.getByRole('progressbar')
  expect(bar.getAttribute('aria-valuenow')).toBe('3')
  expect(bar.getAttribute('aria-valuemax')).toBe('5')

  // children load through the parent_id filter and render as compact rows
  await view.findByText('Child ORB-2')
  expect(urls[0]).toContain('parent_id=task-1')
  const rows = view.container.querySelectorAll('.tasks-row[data-compact="true"]')
  expect(rows.length).toBe(2)
  // only the cross-project child carries a project colour dot
  expect(rows[0].querySelector('.tasks-row-project')).toBeNull()
  expect(rows[1].querySelector('.tasks-row-project')).not.toBeNull()
})

test('a leaf task never requests its (empty) sub-issue list', () => {
  let calls = 0
  globalThis.fetch = (async () => {
    calls += 1
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const leaf = { ...parent, subIssueTotal: 0, subIssueDone: 0 }
  const view = render(<SubIssues task={leaf} projects={[]} state={state} onOpen={() => {}} />, { wrapper: Wrapper })
  expect(view.queryByRole('progressbar')).toBeNull()
  expect(calls).toBe(0)
})

test('the add button opens the create modal preset to this parent', async () => {
  const bodies: unknown[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    if (request.method === 'POST') {
      bodies.push(await request.json())
      return Response.json(child('task-9', 'ORB-9', 'project-1'), { status: 201 })
    }
    return Response.json({ items: [], next_cursor: null })
  }) as unknown as typeof fetch
  const view = render(<SubIssues task={parent} projects={[]} state={state} onOpen={() => {}} />, { wrapper: Wrapper })

  fireEvent.click(view.getByRole('button', { name: 'Add sub-issue' }))
  expect(await view.findByRole('dialog')).toBeTruthy()

  const input = view.getByLabelText('Issue title') as HTMLInputElement
  fireEvent.change(input, { target: { value: 'Write the docs' } })
  fireEvent.click(view.getByRole('button', { name: 'Create sub-issue' }))

  await waitFor(() => expect(bodies.length).toBe(1))
  expect(bodies[0]).toMatchObject({ title: 'Write the docs', parent_id: 'task-1', project_id: 'project-1', status_id: 'todo' })
  // A single create closes the modal instead of navigating anywhere.
  await waitFor(() => expect(view.queryByRole('dialog')).toBeNull())
})

test('the task graph styles are declared in tasks.css', async () => {
  const css = await Bun.file(new URL('../tasks.css', import.meta.url)).text()
  for (const selector of [
    '.tasks-subissues',
    '.tasks-subissues-header',
    '.tasks-subissues-count',
    '.tasks-subissues-bar',
    '.tasks-subissues-bar-fill',
    '.tasks-row[data-compact=',
    '.tasks-row[data-depth=',
    '.tasks-subissues-empty-add',
    '.tasks-row-disclosure',
    '.tasks-duplicate-banner',
    '.tasks-side-link',
  ]) {
    expect(css.includes(selector)).toBe(true)
  }
  expect(css.includes('prefers-reduced-motion')).toBe(true)
})
