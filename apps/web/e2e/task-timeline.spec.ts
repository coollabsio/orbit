import { expect, test } from '@playwright/test'

test.use({ viewport: { width: 1280, height: 800 }, timezoneId: 'Europe/Berlin' })

test('drag a timeline bar, then open the task and come back to the same layout', async ({ page }) => {
  await page.clock.setFixedTime(new Date('2026-09-23T10:00:00+02:00'))
  const task = {
    id: 'task-1', workspace_id: 'alpha', project_id: 'project-1', status_id: 'todo',
    title: 'Roadmap timeline', description: '', priority: 'none', position: 1,
    assignee_ids: [], label_ids: [], creator_id: 'user-1', version: 1,
    // Berlin: 22 Sep 00:00 → 25 Sep 09:00
    due_start_at: '2026-09-21T22:00:00Z', due_at: '2026-09-25T07:00:00Z',
    created_at: '2026-09-01T12:00:00Z', updated_at: '2026-09-01T12:00:00Z',
  }
  const patches: Array<Record<string, unknown>> = []
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request()
    const path = new URL(request.url()).pathname
    if (request.method() === 'PATCH' && path.endsWith('/tasks/task-1')) {
      const body = request.postDataJSON() as Record<string, unknown>
      patches.push(body)
      Object.assign(task, { due_start_at: body.due_start_at, due_at: body.due_at, version: task.version + 1 })
      await route.fulfill({ json: task })
      return
    }
    let body: unknown = { items: [], next_cursor: null }
    if (path.endsWith('/setup/status')) body = { complete: true }
    if (path.endsWith('/auth/me')) body = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
    if (path === '/api/v1/workspaces') body = [{ id: 'alpha', name: 'Alpha', role: 'owner', version: 1 }]
    if (path.endsWith('/projects')) body = { items: [{ id: 'project-1', name: 'Launch', key: 'TEST', color: '#e0457b', version: 1 }], next_cursor: null }
    if (path.endsWith('/statuses')) body = { items: [{ id: 'todo', project_id: 'project-1', name: 'Todo', category: 'unstarted', color: '#888888', position: 0, version: 1 }], next_cursor: null }
    if (path.endsWith('/tasks')) body = { items: [task], next_cursor: null }
    if (path.endsWith('/tasks/task-1')) body = task
    if (path.endsWith('/github-links')) body = []
    await route.fulfill({ json: body })
  })

  await page.goto('/tasks?workspace=alpha&layout=timeline')
  await page.getByRole('button', { name: 'Week' }).click()
  const bar = page.locator('[data-timeline-bar="task-1"]')
  await expect(bar).toBeVisible()

  const box = (await bar.boundingBox())!
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
  await page.mouse.down()
  await page.mouse.move(box.x + box.width / 2 + 3 * 44, box.y + box.height / 2, { steps: 8 })
  await page.mouse.up()

  await expect.poll(() => patches.length).toBe(1)
  expect(patches[0]).toMatchObject({ due_start_at: '2026-09-24T22:00:00.000Z', due_at: '2026-09-28T07:00:00.000Z', expected_version: 1 })

  await bar.click()
  await expect(page).toHaveURL(/\/tasks\/task-1\?.*layout=timeline/)
  await page.keyboard.press('Escape')
  await expect(page).toHaveURL(/\/tasks\?.*layout=timeline/)
  await expect(page.getByRole('button', { name: 'Week' })).toHaveAttribute('aria-pressed', 'true')

  // sidebar links carry no layout param; the timeline the user came in with must stay
  await page.getByRole('link', { name: 'Overdue' }).first().click()
  await expect(page).toHaveURL(/view=overdue/)
  await expect(page.locator('[data-timeline-scroller]')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Week' })).toBeVisible()
})

test.describe('narrow screens', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true })

  test('project and "No dates" toggles stay reachable without the left pane', async ({ page }) => {
    const base = { workspace_id: 'alpha', project_id: 'project-1', status_id: 'todo', description: '', priority: 'none', assignee_ids: [], label_ids: [], creator_id: 'user-1', version: 1, created_at: '2026-09-01T12:00:00Z', updated_at: '2026-09-01T12:00:00Z' }
    const tasks = [
      { ...base, id: 'task-1', title: 'Dated task', position: 1, due_start_at: '2026-09-21T22:00:00Z', due_at: '2026-09-25T07:00:00Z' },
      { ...base, id: 'task-2', title: 'Undated task', position: 2 },
    ]
    await page.route('**/api/v1/**', async (route) => {
      const path = new URL(route.request().url()).pathname
      let body: unknown = { items: [], next_cursor: null }
      if (path.endsWith('/setup/status')) body = { complete: true }
      if (path.endsWith('/auth/me')) body = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
      if (path === '/api/v1/workspaces') body = [{ id: 'alpha', name: 'Alpha', role: 'owner', version: 1 }]
      if (path.endsWith('/projects')) body = { items: [{ id: 'project-1', name: 'Launch', key: 'TEST', color: '#e0457b', version: 1 }], next_cursor: null }
      if (path.endsWith('/statuses')) body = { items: [{ id: 'todo', project_id: 'project-1', name: 'Todo', category: 'unstarted', color: '#888888', position: 0, version: 1 }], next_cursor: null }
      if (path.endsWith('/tasks')) body = { items: tasks, next_cursor: null }
      await route.fulfill({ json: body })
    })
    await page.goto('/tasks?workspace=alpha&layout=timeline')
    await expect(page.getByRole('button', { name: /Launch/ })).toBeVisible()
    await page.getByRole('button', { name: 'No dates (1)' }).tap()
    await expect(page.getByRole('button', { name: /Undated task/ })).toBeVisible()
  })
})
