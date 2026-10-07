import { expect, test } from '@playwright/test'
import { testWorkspace } from '../src/test/workspace'

test('shortcuts navigate, create, and follow a rebinding after a reload', async ({ page }) => {
  // the server side of the shortcuts: one stored map per user
  let bindings: Record<string, string | null> = {}
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/v1/auth/shortcuts') {
      if (route.request().method() === 'PUT') bindings = route.request().postDataJSON().bindings
      await route.fulfill({ json: { bindings } })
      return
    }
    if (/\/views\/[^/]+\/favorite$|\/view-favorites\/order$/.test(path)) { await route.fulfill({ status: 204 }); return }
    let body: unknown = { items: [], next_cursor: null }
    if (path.endsWith('/views')) body = []
    if (path === '/api/v1/setup/status') body = { complete: true }
    if (path === '/api/v1/auth/me') body = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
    if (path === '/api/v1/workspaces') body = [testWorkspace('owner', { id: 'alpha', name: 'Alpha' })]
    await route.fulfill({ json: body })
  })

  await page.goto('/tasks?workspace=alpha')
  await expect(page.getByRole('button', { name: 'New task' })).toBeVisible()

  // a sequence goes to a page
  await page.keyboard.press('g')
  await page.keyboard.press('i')
  await expect(page).toHaveURL(/\/inbox/)
  await page.keyboard.press('g')
  await page.keyboard.press('t')
  await expect(page).toHaveURL(/\/tasks/)

  // C creates a task on every page; Escape closes the dialog
  const title = page.getByLabel('Task title')
  await page.keyboard.press('c')
  await expect(title).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(title).toBeHidden()

  // rebind "Create task" to N
  await page.goto('/settings/shortcuts?workspace=alpha')
  await page.getByRole('button', { name: 'Change shortcut for Create task' }).click()
  await page.keyboard.press('n')
  await expect.poll(() => bindings).toEqual({ 'task.create': 'N' })
  await expect(page.getByRole('button', { name: 'Reset Create task' })).toBeVisible()

  // the new key works after a reload, the old one does not
  await page.goto('/tasks?workspace=alpha')
  await page.getByRole('button', { name: 'New task' }).hover()
  await expect(page.getByText('New task (N)')).toBeVisible()
  await page.keyboard.press('c')
  await expect(title).toBeHidden()
  await page.keyboard.press('n')
  await expect(title).toBeVisible()
})
