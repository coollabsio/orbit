import { expect, test } from '@playwright/test'

for (const touch of [false, true]) {
  test.describe(touch ? 'touch task rows' : 'mouse task rows', () => {
    test.use({ hasTouch: touch, viewport: { width: touch ? 390 : 1280, height: 844 } })

    test.beforeEach(async ({ page }) => {
      const tasks = [1, 2].map((id) => ({
        id: `task-${id}`, workspace_id: 'alpha', project_id: 'project-1', status_id: 'todo',
        title: `Task ${id}`, description: '', priority: 'none', position: id,
        assignee_ids: [], label_ids: [], creator_id: 'user-1', version: 1,
        created_at: '2026-09-01T12:00:00Z', updated_at: '2026-09-01T12:00:00Z',
      }))
      await page.route('**/api/v1/**', async (route) => {
        const path = new URL(route.request().url()).pathname
        let body: unknown = { items: [], next_cursor: null }
        if (path.endsWith('/setup/status')) body = { complete: true }
        if (path.endsWith('/auth/me')) body = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
        if (path === '/api/v1/workspaces') body = [{ id: 'alpha', name: 'Alpha', role: 'owner', version: 1 }]
        if (path.endsWith('/projects')) body = { items: [{ id: 'project-1', name: 'Launch', key: 'TEST', color: '#123456', version: 1 }], next_cursor: null }
        if (path.endsWith('/statuses')) body = { items: [{ id: 'todo', project_id: 'project-1', name: 'Todo', category: 'unstarted', color: '#123456', position: 0, version: 1 }], next_cursor: null }
        if (path.endsWith('/tasks')) body = { items: tasks, next_cursor: null }
        if (path.endsWith('/tasks/task-1')) body = tasks[0]
        await route.fulfill({ json: body })
      })
      await page.goto('/tasks?workspace=alpha')
      await expect(page.locator('.tasks-row')).toHaveCount(2)
    })

    test('one click opens the task', async ({ page }) => {
      const title = page.locator('.tasks-row-title').first()
      if (touch) await title.tap()
      else await title.click()
      await expect(page).toHaveURL(/\/tasks\/task-1\?workspace=alpha$/)
      await expect(page.locator('.tasks-page')).toHaveAttribute('data-view', 'detail')
    })

    test('inline controls do not open the task and Enter still does', async ({ page }) => {
      const row = page.locator('.tasks-row').first()
      for (const name of ['Priority: No priority', 'Status: Todo']) {
        const control = row.getByRole('button', { name, exact: true })
        if (touch) await control.tap()
        else await control.click()
        await expect(page.locator('.popover')).toBeVisible()
        await expect(page).toHaveURL(/\/tasks\?workspace=alpha$/)
        await page.keyboard.press('Escape')
      }
      await row.focus()
      await page.keyboard.press('Enter')
      await expect(page).toHaveURL(/\/tasks\/task-1\?workspace=alpha$/)
    })

    test('the hidden selection gutter selects only its row without opening', async ({ page }) => {
      const rows = page.locator('.tasks-row')
      await expect(rows.first().locator('.checkbox-box-visual')).toHaveCSS('opacity', '0')
      for (let i = 0; i < 2; i++) {
        const bounds = (await rows.nth(i).boundingBox())!
        // The left gutter belongs to selection, including padding outside the visible box.
        if (touch) await page.touchscreen.tap(bounds.x + 5, bounds.y + bounds.height / 2)
        else await page.mouse.click(bounds.x + 5, bounds.y + bounds.height / 2)
        await expect(rows.nth(i).getByRole('checkbox')).toBeChecked()
        await expect(page).toHaveURL(/\/tasks\?workspace=alpha$/)
        if (i === 0) await expect(rows.nth(1).getByRole('checkbox')).not.toBeChecked()
      }
      await rows.first().getByRole('checkbox').uncheck()
      await expect(rows.first().getByRole('checkbox')).not.toBeChecked()
      await expect(rows.nth(1).getByRole('checkbox')).toBeChecked()
    })
  })
}
