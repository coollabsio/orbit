import { expect, test, type Page } from '@playwright/test'
import { testWorkspace } from '../src/test/workspace'

test.use({ viewport: { width: 1280, height: 800 } })

type Write = { method: string; path: string; body: unknown }

const task = (index: number) => ({
  id: `task-000${index}`, title: `Task ${index}`, workspace_id: 'alpha', project_id: 'project-1', status_id: 'todo', description: '',
  priority: 'none', assignee_ids: [], label_ids: [], creator_id: 'user-1', blocked: false, duplicate_of: null, parent_task_id: null,
  parent: null, sub_issue_count: 0, sub_issue_closed_count: 0, position: index, version: 1, deleted_at: null,
  created_at: `2026-09-27T10:00:0${index}Z`, updated_at: '2026-09-27T12:00:00Z',
})

async function mockApi(page: Page, writes: Write[]) {
  const tasks = [task(1), task(2), task(3)]
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request()
    const path = new URL(request.url()).pathname
    // task queries are reads sent as POST: keep them out of the recorded writes
    if (request.method() !== 'GET' && !path.endsWith('/tasks/query') && request.postData()) writes.push({ method: request.method(), path, body: request.postDataJSON() })
    let body: unknown = { items: [], next_cursor: null }
    if (path.endsWith('/views')) body = []
    if (path.endsWith('/setup/status')) body = { complete: true }
    if (path.endsWith('/auth/me')) body = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
    if (path === '/api/v1/workspaces') body = [testWorkspace('owner', { id: 'alpha', name: 'Alpha' })]
    if (path.endsWith('/projects')) body = { items: [{ id: 'project-1', name: 'Launch', key: 'TEST', color: '#e0457b', version: 1 }], next_cursor: null }
    if (path.endsWith('/statuses')) {
      body = { items: [
        { id: 'todo', project_id: 'project-1', name: 'Todo', description: '', category: 'unstarted', color: '#888888', position: 0, version: 1 },
        { id: 'done', project_id: 'project-1', name: 'Done', description: '', category: 'completed', color: '#4cb782', position: 1, version: 1 },
      ], next_cursor: null }
    }
    if (path.endsWith('/tasks') || path.endsWith('/tasks/query') || path.endsWith('/tasks/bulk')) body = { items: tasks, next_cursor: null }
    await route.fulfill({ json: body })
  })
}

test('a right-click on a task row opens the menu and a submenu choice updates the task', async ({ page }) => {
  const writes: Write[] = []
  await mockApi(page, writes)
  await page.goto('/tasks?workspace=alpha')

  await page.locator('[data-task-row]', { hasText: 'Task 2' }).click({ button: 'right' })
  const menu = page.getByRole('menu')
  await expect(menu.getByRole('menuitem', { name: 'Move to trash' })).toBeVisible()
  await menu.getByRole('menuitem', { name: /Priority/ }).hover()
  await page.getByRole('menuitemcheckbox', { name: 'Urgent' }).click()

  await expect.poll(() => writes.length).toBe(1)
  expect(writes[0].body).toEqual({ updates: [{ id: 'task-0002', expected_version: 1, priority: 'urgent' }] })
  await expect(page.getByRole('menu')).toHaveCount(0)
})

test('a right-click on another row while the menu is open moves the menu to that row', async ({ page }) => {
  const writes: Write[] = []
  await mockApi(page, writes)
  await page.goto('/tasks?workspace=alpha')

  await page.locator('[data-task-row]', { hasText: 'Task 2' }).click({ button: 'right' })
  await expect(page.getByRole('menuitem', { name: 'Move to trash' })).toBeVisible()
  const row = (await page.locator('[data-task-row]', { hasText: 'Task 3' }).boundingBox())!
  await page.mouse.click(row.x + 300, row.y + row.height / 2, { button: 'right' })
  await page.getByRole('menuitem', { name: /Priority/ }).hover()
  await page.getByRole('menuitemcheckbox', { name: 'Urgent' }).click()

  await expect.poll(() => writes.length).toBe(1)
  expect(writes[0].body).toEqual({ updates: [{ id: 'task-0003', expected_version: 1, priority: 'urgent' }] })
})

test('a right-click on a group header leaves the browser menu alone', async ({ page }) => {
  await mockApi(page, [])
  await page.goto('/tasks?workspace=alpha')
  await page.locator('[data-slot="group-header"]').first().click({ button: 'right' })
  await expect(page.getByRole('menu')).toHaveCount(0)
})
