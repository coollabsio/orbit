import { expect, test } from '@playwright/test'

test.use({ viewport: { width: 390, height: 740 }, isMobile: true, hasTouch: true })

test('mobile login fields do not trigger Safari small-input zoom', async ({ page }) => {
  await page.goto('/login')
  for (const label of ['Email', 'Password']) {
    const input = page.getByLabel(label, { exact: true })
    const fontSize = await input.evaluate((element) => parseFloat(getComputedStyle(element).fontSize))
    expect(fontSize).toBeGreaterThanOrEqual(16)
  }
})

test('login dismisses field focus and shows both toolbars without refreshing', async ({ page }) => {
  let finishLogin!: () => void
  const loginResponse = new Promise<void>((resolve) => { finishLogin = resolve })
  let loggedIn = false
  const user = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    let body: unknown = { items: [], next_cursor: null }
    if (path === '/api/v1/auth/login') {
      await loginResponse
      loggedIn = true
      body = { user }
    }
    if (path === '/api/v1/setup/status') body = { complete: true }
    if (path === '/api/v1/auth/me') {
      if (!loggedIn) { await route.fulfill({ status: 401, json: { status: 401, code: 'authentication_required' } }); return }
      body = user
    }
    if (path === '/api/v1/workspaces') body = [{ id: 'alpha', name: 'Alpha', role: 'owner', version: 1 }]
    await route.fulfill({ json: body })
  })
  await page.goto('/login')
  await page.getByLabel('Email', { exact: true }).fill('test@example.com')
  const password = page.getByLabel('Password', { exact: true })
  await password.fill('password')
  // Model the smaller viewport while a mobile keyboard is open.
  await page.setViewportSize({ width: 390, height: 400 })
  await password.press('Enter')
  await expect(page.getByRole('button', { name: 'Please wait…' })).toBeVisible()
  await expect(password).not.toBeFocused()
  await page.setViewportSize({ width: 390, height: 740 })
  // Keep a document offset available, modelling Safari's retained login pan.
  await page.addStyleTag({ content: 'html { min-height: calc(100% + 160px); }' })
  await page.evaluate(() => window.scrollTo(0, 100))
  expect(await page.evaluate(() => window.scrollY)).toBe(100)
  finishLogin()
  await expect(page).toHaveURL(/\/tasks$/)
  const menu = page.locator('.tasks-sidebar-button')
  const dock = page.getByRole('navigation', { name: 'Mobile navigation' })
  await expect(menu).toBeInViewport({ ratio: 1 })
  await expect(dock).toBeInViewport({ ratio: 1 })
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0)
  await menu.click()
  await expect(page.locator('.mobile-drawer')).toBeVisible()
})
