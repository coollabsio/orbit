import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { WorkspaceRecord } from '../../../api/generated/types.gen'
import { ConfirmationModalHost } from '../../../components/ui/ConfirmationModal'
import { WorkspaceContext } from '../../workspaces/workspaceContext'
import type { Task } from '../api/models'
import { DuplicateBanner, DuplicateErrors, DuplicateMenuItems, DuplicatesGroup, MarkDuplicateDialog, useDuplicateFlow } from './TaskDuplicates'

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
          <ConfirmationModalHost />
        </WorkspaceContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>
  )
}

function task(id: string, identifier: string, overrides: Partial<Task> = {}): Task {
  return {
    id, identifier, title: `Title ${identifier}`, descriptionJson: { type: 'doc', content: [] }, descriptionText: '',
    statusId: 's', position: 0, priority: 'none', assigneeIds: [], creatorId: 'u', projectId: 'p', labels: [],
    attachments: [], dueAt: null, createdAt: '', updatedAt: '', comments: [], activity: [], version: 1,
    parentId: null, subIssueTotal: 0, subIssueDone: 0, duplicateOfTaskId: null, duplicateIds: [], referencedBy: [],
    ...overrides,
  }
}

/** The pieces as TaskDetail wires them: menu items, the picker and the alerts share one flow. */
function DuplicateHarness({ subject, close = () => {} }: { subject: Task; close?: () => void }) {
  const flow = useDuplicateFlow(subject)
  return (
    <>
      <DuplicateMenuItems task={subject} flow={flow} close={close} />
      <MarkDuplicateDialog task={subject} flow={flow} workspaceId={workspace.id} />
      <DuplicateErrors flow={flow} />
    </>
  )
}

test('a duplicate shows a muted banner linking to the canonical task', () => {
  const duplicate = task('task-1', 'ORB-1', { duplicateOfTaskId: 'task-9' })
  const view = render(<DuplicateBanner task={duplicate} tasks={[duplicate, task('task-9', 'ORB-9')]} />, { wrapper: Wrapper })
  const link = view.getByRole('link', { name: 'Duplicate of ORB-9' })
  expect(link.getAttribute('href')).toBe('/tasks/task-9')
})

test('a task that is not a duplicate has no banner', () => {
  const view = render(<DuplicateBanner task={task('task-1', 'ORB-1')} tasks={[]} />, { wrapper: Wrapper })
  expect(view.queryByRole('note')).toBeNull()
})

test('a canonical task that is off the page is fetched for the banner', async () => {
  globalThis.fetch = (async () => Response.json({
    id: 'task-9', workspace_id: 'workspace-1', project_id: 'p', status_id: 's', identifier: 'ORB-9', identifier_key: 'ORB',
    number: 9, title: 'Canonical', description_json: { type: 'doc', content: [] }, description_text: '', position: 0,
    priority: 'none', assignee_ids: [], creator_id: 'u', label_ids: [], sub_issue_total: 0, sub_issue_done: 0,
    duplicate_ids: ['task-1'], referenced_by: [], created_at: '', updated_at: '', version: 0,
  })) as unknown as typeof fetch
  const duplicate = task('task-1', 'ORB-1', { duplicateOfTaskId: 'task-9' })
  const view = render(<DuplicateBanner task={duplicate} tasks={[duplicate]} />, { wrapper: Wrapper })
  expect(await view.findByRole('link', { name: 'Duplicate of ORB-9' })).toBeTruthy()
})

test('the canonical task lists its duplicates and opens them', () => {
  const canonical = task('task-9', 'ORB-9', { duplicateIds: ['task-1'] })
  const onOpen = mock((_taskId: string) => {})
  const view = render(<DuplicatesGroup task={canonical} tasks={[canonical, task('task-1', 'ORB-1')]} onOpen={onOpen} />, { wrapper: Wrapper })
  expect(view.getByText('Duplicates')).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'ORB-1' }))
  expect(onOpen).toHaveBeenCalledWith('task-1')
})

test('marking picks a target with the @ search, confirms once, then posts', async () => {
  const requests: { method: string; url: string; body: unknown }[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    requests.push({ method: request.method, url: request.url, body: request.method === 'POST' ? await request.json() : null })
    if (request.method === 'POST') return Response.json({ id: 'task-1', duplicate_of_task_id: 'task-9', version: 2 })
    return Response.json({ items: [{ id: 'task-9', identifier: 'ORB-9', title: 'Gateway rewrite' }], next_cursor: null })
  }) as unknown as typeof fetch
  const close = mock(() => {})
  const view = render(<DuplicateHarness subject={task('task-1', 'ORB-1')} close={close} />, { wrapper: Wrapper })

  expect(view.queryByText(/merge/i)).toBeNull()
  fireEvent.click(view.getByRole('button', { name: 'Mark as duplicate…' }))
  expect(close).toHaveBeenCalledTimes(1)
  const dialog = await view.findByRole('dialog')
  expect(dialog.textContent).not.toMatch(/merge/i)

  fireEvent.change(view.getByRole('textbox', { name: 'Search issues' }), { target: { value: 'gate' } })
  fireEvent.mouseDown(await view.findByRole('option', { name: /ORB-9/ }))

  // one light confirmation, and it says what happens to the status
  const confirm = await view.findByRole('button', { name: 'Mark as duplicate' })
  expect(view.getByText(/does not restore the previous status/)).toBeTruthy()
  expect(requests.some((request) => request.method === 'POST')).toBe(false)
  fireEvent.click(confirm)

  await waitFor(() => expect(requests.filter((request) => request.method === 'POST')).toHaveLength(1))
  const post = requests.find((request) => request.method === 'POST')
  expect(post?.url).toContain('/tasks/task-1/duplicate-of')
  expect(post?.body).toEqual({ target_task_id: 'task-9', expected_version: 1 })
})

test('cancelling the confirmation posts nothing', async () => {
  const methods: string[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    methods.push((input as Request).method)
    return Response.json({ items: [{ id: 'task-9', identifier: 'ORB-9', title: 'Gateway rewrite' }], next_cursor: null })
  }) as unknown as typeof fetch
  const view = render(<DuplicateHarness subject={task('task-1', 'ORB-1')} />, { wrapper: Wrapper })

  fireEvent.click(view.getByRole('button', { name: 'Mark as duplicate…' }))
  fireEvent.change(await view.findByRole('textbox', { name: 'Search issues' }), { target: { value: 'gate' } })
  fireEvent.mouseDown(await view.findByRole('option', { name: /ORB-9/ }))
  fireEvent.click(await view.findByRole('button', { name: 'Cancel' }))

  await waitFor(() => expect(view.queryByRole('dialog')).toBeNull())
  expect(methods.includes('POST')).toBe(false)
})

test('a marked task offers "Remove duplicate mark", which deletes the relation', async () => {
  const calls: string[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    calls.push((input as Request).method)
    return new Response(null, { status: 204 })
  }) as unknown as typeof fetch
  const marked = task('task-1', 'ORB-1', { duplicateOfTaskId: 'task-9' })
  const view = render(<DuplicateHarness subject={marked} />, { wrapper: Wrapper })

  expect(view.queryByText(/merge/i)).toBeNull()
  expect(view.queryByRole('button', { name: 'Mark as duplicate…' })).toBeNull()
  fireEvent.click(view.getByRole('button', { name: 'Remove duplicate mark' }))
  await waitFor(() => expect(calls).toEqual(['DELETE']))
})
