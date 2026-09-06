import { expect, test } from '@playwright/test'

test('Tasks opens the mobile sidebar with workspace selection without an extra header row', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  const workspaces = [
    { id: 'alpha', name: 'Alpha', role: 'owner', version: 1 },
    { id: 'beta', name: 'Beta', role: 'owner', version: 1 },
  ]
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    let body: unknown = { items: [], next_cursor: null }
    if (path === '/api/v1/setup/status') body = { complete: true }
    if (path === '/api/v1/auth/me') body = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
    if (path === '/api/v1/workspaces') body = workspaces
    if (path === '/api/v1/workspaces' && route.request().method() === 'POST') {
      expect(route.request().postDataJSON()).toEqual({ name: 'Gamma' })
      const created = { id: 'gamma', name: 'Gamma', role: 'owner', version: 1 }
      workspaces.push(created)
      body = created
    }
    if (path === '/api/v1/workspaces/gamma' && route.request().method() === 'PATCH') {
      expect(route.request().postDataJSON()).toEqual({ name: 'Gamma renamed', expected_version: 1 })
      body = { id: 'gamma', name: 'Gamma renamed', role: 'owner', version: 2 }
    }
    await route.fulfill({ json: body })
  })
  await page.goto('/tasks?workspace=alpha')
  const menu = page.getByRole('button', { name: 'Menu', exact: true })
  await expect(menu).toBeVisible()
  await expect(page.getByRole('button', { name: 'Workspace: Alpha' })).not.toBeVisible()
  await menu.click()
  const drawer = page.locator('.mobile-drawer')
  await drawer.getByRole('button', { name: 'Workspace: Alpha' }).click()
  await page.getByRole('button', { name: 'Beta', exact: true }).click()
  await expect(page).toHaveURL(/workspace=beta/)
  await expect(drawer).not.toBeVisible()
  await menu.click()
  await expect(drawer.getByRole('button', { name: 'Workspace: Beta' })).toBeVisible()
  await drawer.getByRole('link', { name: 'Settings', exact: true }).click()
  await expect(drawer).not.toBeVisible()
  await page.getByRole('button', { name: 'Menu', exact: true }).click()
  await expect(drawer.getByRole('button', { name: 'Workspace: Beta' })).toBeVisible()
  await drawer.getByRole('button', { name: 'Workspace: Beta' }).click()
  await page.getByRole('button', { name: 'Create workspace', exact: true }).click()
  await page.getByLabel('New workspace').fill('Gamma')
  await page.getByRole('button', { name: 'Create workspace', exact: true }).click()
  await expect(page).toHaveURL(/workspace=gamma/)
  await expect(drawer).not.toBeVisible()
  await expect(page.getByLabel('New workspace')).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Create workspace', exact: true })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Save workspace' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Save Changes' })).toHaveCount(0)
  await page.getByLabel('Name', { exact: true }).fill('Draft name')
  await expect(page.getByRole('button', { name: 'Save Changes' })).toBeVisible()
  const popup = page.locator('.unsaved-bar')
  await expect.poll(async () => Math.round((await popup.boundingBox())?.y ?? -1)).toBe(12)
  await page.evaluate(() => {
    Object.defineProperty(window.visualViewport, 'height', { configurable: true, value: 440 })
    Object.defineProperty(window.visualViewport, 'offsetTop', { configurable: true, value: 180 })
    Object.defineProperty(window.visualViewport, 'offsetLeft', { configurable: true, value: 20 })
    Object.defineProperty(window.visualViewport, 'width', { configurable: true, value: 350 })
    window.visualViewport!.dispatchEvent(new Event('resize'))
  })
  await page.locator('.settings-scroll').evaluate((element) => { element.scrollTop = 240 })
  await expect.poll(async () => Math.round((await popup.boundingBox())?.y ?? -1)).toBe(192)
  await expect.poll(async () => Math.round((await popup.boundingBox())?.x ?? -1)).toBe(32)
  await expect.poll(async () => Math.round((await popup.boundingBox())?.width ?? -1)).toBe(326)
  await page.evaluate(() => {
    Object.defineProperty(window.visualViewport, 'offsetTop', { configurable: true, value: 240 })
    window.visualViewport!.dispatchEvent(new Event('scroll'))
  })
  await expect.poll(async () => Math.round((await popup.boundingBox())?.y ?? -1)).toBe(252)
  await page.evaluate(() => {
    delete (window.visualViewport as unknown as { height?: number }).height
    delete (window.visualViewport as unknown as { offsetTop?: number }).offsetTop
    delete (window.visualViewport as unknown as { offsetLeft?: number }).offsetLeft
    delete (window.visualViewport as unknown as { width?: number }).width
    window.visualViewport!.dispatchEvent(new Event('resize'))
  })
  await page.setViewportSize({ width: 1280, height: 844 })
  await expect(popup).toHaveCSS('position', 'absolute')
  await expect(popup).toHaveCSS('bottom', '20px')
  await expect.poll(async () => (await popup.boundingBox())!.y).toBeGreaterThan(600)
  await page.setViewportSize({ width: 390, height: 844 })
  await expect.poll(async () => Math.round((await popup.boundingBox())?.y ?? -1)).toBe(12)

  await page.getByRole('button', { name: 'Reset', exact: true }).click()
  await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Gamma')
  await expect(page.getByRole('button', { name: 'Save Changes' })).toHaveCount(0)
  await page.getByLabel('Name', { exact: true }).fill(' Gamma renamed ')
  await page.getByRole('button', { name: 'Save Changes' }).click()
  await expect(page.getByRole('button', { name: 'Save Changes' })).toHaveCount(0)
  await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Gamma renamed')
  await page.setViewportSize({ width: 1280, height: 844 })
  await expect(page.locator('.app-sidebar').getByRole('button', { name: 'Workspace: Gamma renamed' })).toBeVisible()
})
