import { expect, test } from '@playwright/test'

for (const width of [390, 1280]) {
  test(`workspace deletion requires confirmation at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 844 })
    let deleted = false
    let finishDeletion!: () => void
    const pendingDeletion = new Promise<void>((resolve) => { finishDeletion = resolve })
    const workspaces = [
      { id: 'alpha', name: 'Alpha', role: 'owner', version: 7 },
      { id: 'beta', name: 'Beta', role: 'owner', version: 1 },
    ]
    await page.route('**/api/v1/**', async (route) => {
      const url = new URL(route.request().url())
      let body: unknown = { items: [], next_cursor: null }
      if (url.pathname === '/api/v1/setup/status') body = { complete: true }
      if (url.pathname === '/api/v1/auth/me') body = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
      if (url.pathname === '/api/v1/workspaces') body = workspaces.filter((item) => !deleted || item.id !== 'alpha')
      if (route.request().method() === 'DELETE') {
        expect(url.pathname).toBe('/api/v1/workspaces/alpha')
        expect(url.searchParams.get('expected_version')).toBe('7')
        await pendingDeletion
        deleted = true
        await route.fulfill({ status: 204 })
        return
      }
      await route.fulfill({ json: body })
    })
    await page.goto('/settings?workspace=alpha')
    await expect(page.getByRole('button', { name: 'Delete workspace', exact: true })).toHaveCount(0)
    await page.getByRole('navigation', { name: 'Settings', exact: true }).getByRole('link', { name: 'Danger zone' }).click()
    await expect(page).toHaveURL(/\/settings\/danger-zone/)
    await page.getByRole('button', { name: 'Delete workspace', exact: true }).click()
    const dialog = page.getByRole('dialog', { name: 'Delete workspace?' })
    await expect(dialog).toBeVisible()
    await expect(dialog.getByRole('button', { name: 'Delete workspace', exact: true })).toBeDisabled()
    await dialog.getByRole('button', { name: 'Cancel' }).click()
    expect(deleted).toBe(false)
    await page.getByRole('button', { name: 'Delete workspace', exact: true }).click()
    await dialog.getByLabel('Confirm workspace name').fill('Alpha')
    await dialog.getByRole('button', { name: 'Delete workspace', exact: true }).click()
    await expect(dialog.getByRole('button', { name: 'Deleting…' })).toHaveAttribute('aria-disabled', 'true')
    expect(await dialog.evaluate((element) => element.contains(document.activeElement))).toBe(true)
    await page.keyboard.press('Tab')
    expect(await dialog.evaluate((element) => element.contains(document.activeElement))).toBe(true)
    await page.keyboard.press('Escape')
    await expect(dialog).toBeVisible()
    finishDeletion()
    await expect(dialog).not.toBeVisible()
    await expect(page).toHaveURL(/\/settings\?workspace=beta$/)
    await page.reload()
    await expect(page.getByRole('button', { name: 'Delete workspace', exact: true })).toHaveCount(0)
    await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Beta')
  })
}
