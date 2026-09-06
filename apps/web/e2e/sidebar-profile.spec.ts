import { expect, test } from '@playwright/test'

for (const mode of ['desktop', 'collapsed', 'mobile']) {
  test(`${mode} sidebar shows the signed-in profile and supports logout retry`, async ({ page }) => {
    await page.setViewportSize({ width: mode === 'mobile' ? 390 : 1280, height: 844 })
    let signedIn = true
    let attempts = 0
    let finishLogout!: () => void
    await page.route('**/api/v1/**', async (route) => {
      const path = new URL(route.request().url()).pathname
      if (path === '/api/v1/auth/logout') {
        expect(route.request().method()).toBe('POST')
        attempts++
        if (attempts === 1) {
          await route.fulfill({ status: 503, json: { detail: 'Please try again.' } })
          return
        }
        await new Promise<void>((resolve) => { finishLogout = resolve })
        signedIn = false
        await route.fulfill({ status: 204 })
        return
      }
      if (path === '/api/v1/auth/me' && !signedIn) {
        await route.fulfill({ status: 401, json: { code: 'authentication_required' } })
        return
      }
      let body: unknown = { items: [], next_cursor: null }
      if (path === '/api/v1/setup/status') body = { complete: true }
      if (path === '/api/v1/auth/me') body = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
      if (path === '/api/v1/workspaces') body = [{ id: 'alpha', name: 'Alpha', role: 'owner', version: 1 }]
      await route.fulfill({ json: body })
    })
    await page.goto('/tasks?workspace=alpha')
    if (mode === 'mobile') await page.getByRole('button', { name: 'Menu', exact: true }).click()
    if (mode === 'collapsed') await page.getByRole('button', { name: 'Collapse sidebar' }).click()
    const sidebar = page.locator(mode === 'mobile' ? '.mobile-drawer' : '.app-sidebar')
    const trigger = sidebar.getByRole('button', { name: 'Account menu for Test User' })
    await expect(trigger).toBeVisible()
    if (mode !== 'collapsed') await expect(trigger).toContainText('test@example.com')
    const box = await trigger.boundingBox()
    expect(box!.y).toBeGreaterThan(700)
    await trigger.click()
    await page.keyboard.press('Escape')
    await expect(trigger).toHaveAttribute('aria-expanded', 'false')
    await trigger.click()
    const logout = page.getByRole('button', { name: 'Log out', exact: true })
    await expect(logout).toBeVisible()
    const popup = page.locator('.user-menu .popover')
    expect((await popup.boundingBox())!.width).toBeLessThan(250)
    await logout.click()
    await expect(page.getByRole('alert')).toContainText('Could not log out')
    await expect(page).toHaveURL(/tasks/)
    await logout.click()
    await expect(page.getByRole('button', { name: 'Logging out…' })).toBeDisabled()
    await expect.poll(() => attempts).toBe(2)
    finishLogout()
    await expect(page).toHaveURL(/\/login$/)
    await expect(page.getByRole('heading', { name: 'Sign in to Orbit' })).toBeVisible()
    expect(attempts).toBe(2)
    await page.goto('/tasks?workspace=alpha')
    await expect(page).toHaveURL(/\/login$/)
  })
}
