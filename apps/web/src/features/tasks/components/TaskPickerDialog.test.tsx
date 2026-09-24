import { afterEach, expect, mock, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import type { Task, TaskStatusDef } from '@/features/tasks/api/models'
import { TaskPickerDialog, type TaskPickerDialogProps } from './TaskPickerDialog'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const workspace = { id: 'workspace-1', name: 'Orbit', role: 'owner' as const, version: 1 }
const project = { id: 'project-1', workspace_id: 'workspace-1', name: 'Launch', key: 'ORB', color: '#e0457b', created_at: '', updated_at: '', version: 1 }
const statuses: TaskStatusDef[] = [{ id: 'todo', projectId: 'project-1', name: 'Todo', description: '', color: '#888', category: 'unstarted', position: 0, version: 1 }]
const record = (id: string, title: string, duplicateOf: string | null = null) => ({
  id, workspace_id: 'workspace-1', project_id: 'project-1', status_id: 'todo', title, description: '', position: 0,
  priority: 'none', assignee_ids: [], creator_id: 'user-1', label_ids: [], created_at: '', updated_at: '', version: 1,
  duplicate_of: duplicateOf ? { id: duplicateOf, project_id: 'project-1', title: 'Login fails on Safari' } : null, blocked: false,
})
const records = [
  record('task-3f2a', "Can't log in on iPad"),
  record('task-91c0', 'Login fails on Safari'),
  record('task-0b9e', 'Safari login loop', 'task-91c0'),
]

function renderPicker(props: Partial<TaskPickerDialogProps> = {}) {
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = new URL((input as Request).url)
    if (url.pathname.endsWith('/projects')) return Response.json({ items: [project], next_cursor: null })
    // like the server: search matches titles and descriptions, never identifiers
    const search = url.searchParams.get('search')?.toLowerCase()
    return Response.json({ items: search ? records.filter((r) => r.title.toLowerCase().includes(search)) : records, next_cursor: null })
  }) as unknown as typeof fetch
  const onSelect = mock((_task: Task) => {})
  const onOpenChange = mock((_open: boolean) => {})
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>
        <TaskPickerDialog open onOpenChange={onOpenChange} title="Mark ORB-3F2A as duplicate of…" statuses={statuses}
          excludeIds={['task-3f2a']} excludeDuplicates onSelect={onSelect} {...props} />
      </WorkspaceContext.Provider>
    </QueryClientProvider>,
  )
  return { view, onSelect, onOpenChange }
}

test('lists candidates with identifier and project, never the task itself or existing duplicates', async () => {
  const { view } = renderPicker()
  expect(view.getByRole('dialog', { name: 'Mark ORB-3F2A as duplicate of…' })).toBeTruthy()
  const option = await view.findByRole('option', { name: /Login fails on Safari/ })
  expect(option.textContent).toContain('ORB-91C0')
  expect(option.textContent).toContain('Launch')
  expect(view.queryByRole('option', { name: /Can't log in on iPad/ })).toBeNull()
  expect(view.queryByRole('option', { name: /Safari login loop/ })).toBeNull()
})

test('duplicates stay selectable when the caller allows them (blocks / related)', async () => {
  const { view } = renderPicker({ excludeDuplicates: false })
  expect(await view.findByRole('option', { name: /Safari login loop/ })).toBeTruthy()
})

test('an identifier finds its task although the server search ignores identifiers', async () => {
  const { view } = renderPicker()
  await userEvent.type(await view.findByPlaceholderText('Search tasks…'), 'orb-91')
  expect(await view.findByRole('option', { name: /Login fails on Safari/ })).toBeTruthy()
})

test('picking a task reports it and closes the picker', async () => {
  const { view, onSelect, onOpenChange } = renderPicker()
  fireEvent.click(await view.findByRole('option', { name: /Login fails on Safari/ }))
  expect(onSelect.mock.calls[0]![0].id).toBe('task-91c0')
  expect(onOpenChange).toHaveBeenCalledWith(false)
})

test('text that matches nothing shows the empty state', async () => {
  const { view } = renderPicker()
  await userEvent.type(await view.findByPlaceholderText('Search tasks…'), 'zzzz')
  expect(await view.findByText('No matching tasks')).toBeTruthy()
})
