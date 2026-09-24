import { expect, test, type Page } from '@playwright/test'

test.use({ viewport: { width: 1280, height: 800 } })

type DuplicateRef = { id: string; project_id: string; title: string }
type TaskMock = Record<string, unknown> & { id: string; title: string; project_id: string; status_id: string; version: number; duplicate_of: DuplicateRef | null }
type Write = { method: string; path: string; body: unknown }

const base = {
  workspace_id: 'alpha', project_id: 'project-1', description: '', priority: 'none', assignee_ids: [], label_ids: [],
  creator_id: 'user-1', blocked: false, created_at: '2026-09-01T12:00:00Z', updated_at: '2026-09-01T12:00:00Z',
}

function seed(): TaskMock[] {
  return [
    { ...base, id: 'task-91c0', title: 'Login fails on Safari', status_id: 'todo', position: 1, version: 1, duplicate_of: null },
    { ...base, id: 'task-3f2a', title: "Can't log in on iPad", status_id: 'todo', position: 2, version: 1, duplicate_of: null },
    { ...base, id: 'task-0b9e', title: 'Safari login loop', status_id: 'todo', position: 3, version: 1, duplicate_of: null },
  ]
}

/** Server rules in miniature: mark → Duplicate status + relation; unmark → previous status. */
function applyDuplicate(tasks: TaskMock[], id: string, targetId: string | null): TaskMock {
  const task = tasks.find((item) => item.id === id)!
  const target = targetId ? tasks.find((item) => item.id === targetId)! : null
  Object.assign(task, {
    status_id: target ? 'duplicate' : 'todo',
    duplicate_of: target ? { id: target.id, project_id: target.project_id, title: target.title } : null,
    version: task.version + 1,
  })
  return task
}

async function mockApi(page: Page, tasks: TaskMock[], writes: Write[]) {
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request()
    const method = request.method()
    const path = new URL(request.url()).pathname
    if (method === 'PATCH' && /\/tasks\/[^/]+$/.test(path)) {
      const body = request.postDataJSON() as { duplicate_of_id?: string | null }
      writes.push({ method, path, body })
      await route.fulfill({ json: applyDuplicate(tasks, path.split('/').at(-1)!, body.duplicate_of_id ?? null) })
      return
    }
    if (method === 'POST' && path.endsWith('/tasks/bulk')) {
      const body = request.postDataJSON() as { updates: Array<{ id: string; duplicate_of_id: string | null }> }
      writes.push({ method, path, body })
      await route.fulfill({ json: { items: body.updates.map((update) => applyDuplicate(tasks, update.id, update.duplicate_of_id)), next_cursor: null } })
      return
    }
    let body: unknown = { items: [], next_cursor: null }
    if (path.endsWith('/setup/status')) body = { complete: true }
    if (path.endsWith('/auth/me')) body = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
    if (path === '/api/v1/workspaces') body = [{ id: 'alpha', name: 'Alpha', role: 'owner', version: 1 }]
    if (path.endsWith('/projects')) body = { items: [{ id: 'project-1', name: 'Launch', key: 'TEST', color: '#e0457b', version: 1 }], next_cursor: null }
    if (path.endsWith('/statuses')) {
      body = { items: [
        { id: 'todo', project_id: 'project-1', name: 'Todo', description: '', category: 'unstarted', color: '#888888', position: 0, version: 1 },
        { id: 'duplicate', project_id: 'project-1', name: 'Duplicate', description: '', category: 'duplicate', color: '#8b8f98', position: 1, version: 1 },
      ], next_cursor: null }
    }
    if (path.endsWith('/tasks')) body = { items: tasks, next_cursor: null }
    const detail = path.match(/\/tasks\/([^/]+)$/)
    if (detail) body = tasks.find((task) => task.id === detail[1]) ?? body
    const relations = path.match(/\/tasks\/([^/]+)\/relations$/)
    if (relations) {
      const task = tasks.find((item) => item.id === relations[1])
      body = task?.duplicate_of
        ? [{ id: `rel-${task.id}`, type: 'duplicate', direction: 'outgoing', task: { ...task.duplicate_of, status_id: 'todo' }, created_at: '2026-09-23T10:00:00Z' }]
        : []
    }
    if (path.endsWith('/github-links')) body = []
    await route.fulfill({ json: body })
  })
}

test('mark a task as duplicate from its detail page, see the banner, then undo', async ({ page }) => {
  const tasks = seed()
  const writes: Write[] = []
  await mockApi(page, tasks, writes)
  await page.goto('/tasks/task-3f2a?workspace=alpha')

  await page.getByRole('button', { name: /Todo$/ }).click()
  await page.getByRole('menuitem', { name: /Duplicate$/ }).click()
  const picker = page.getByRole('dialog', { name: 'Mark TEST-3F2A as duplicate of…' })
  await expect(picker).toBeVisible()
  await picker.getByPlaceholder('Search tasks…').fill('login fails')
  await expect(picker.getByRole('option', { name: /Can't log in on iPad/ })).toHaveCount(0)
  await picker.getByRole('option', { name: /Login fails on Safari/ }).click()

  const banner = page.getByRole('note', { name: 'Duplicate of TEST-91C0' })
  await expect(banner).toContainText('Login fails on Safari')
  expect(writes[0]).toMatchObject({ method: 'PATCH', body: { expected_version: 1, duplicate_of_id: 'task-91c0' } })

  await expect(page.getByText('Marked as duplicate of TEST-91C0')).toBeVisible()
  await page.getByRole('button', { name: 'Undo' }).click()
  await expect.poll(() => writes.length).toBe(2)
  expect(writes[1]).toMatchObject({ method: 'PATCH', body: { expected_version: 2, duplicate_of_id: null } })
  await expect(banner).toHaveCount(0)
})

test('bulk mark two tasks as duplicates of a third, then undo both', async ({ page }) => {
  const tasks = seed()
  const writes: Write[] = []
  await mockApi(page, tasks, writes)
  await page.goto('/tasks?workspace=alpha')

  await page.getByRole('checkbox', { name: 'Select TEST-3F2A' }).click()
  await page.getByRole('checkbox', { name: 'Select TEST-0B9E' }).click()
  const toolbar = page.getByRole('toolbar', { name: 'Selected tasks' })
  await toolbar.getByRole('button', { name: 'Mark as duplicate…' }).click()
  const picker = page.getByRole('dialog', { name: 'Mark 2 tasks as duplicate of…' })
  await expect(picker.getByRole('option', { name: /Safari login loop/ })).toHaveCount(0)
  await picker.getByRole('option', { name: /Login fails on Safari/ }).click()

  await expect.poll(() => writes.length).toBe(1)
  expect(writes[0]).toMatchObject({ method: 'POST', body: { updates: [
    { id: 'task-3f2a', expected_version: 1, duplicate_of_id: 'task-91c0' },
    { id: 'task-0b9e', expected_version: 1, duplicate_of_id: 'task-91c0' },
  ] } })
  await expect(page.getByRole('button', { name: 'Collapse Duplicate' })).toBeVisible()

  await expect(page.getByText('Marked 2 tasks as duplicate of TEST-91C0')).toBeVisible()
  await page.getByRole('button', { name: 'Undo' }).click()
  await expect.poll(() => writes.length).toBe(2)
  expect(writes[1]).toMatchObject({ method: 'POST', body: { updates: [
    { id: 'task-3f2a', expected_version: 2, duplicate_of_id: null },
    { id: 'task-0b9e', expected_version: 2, duplicate_of_id: null },
  ] } })
  await expect(page.getByRole('button', { name: 'Collapse Duplicate' })).toHaveCount(0)
})
