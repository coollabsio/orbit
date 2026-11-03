# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: mobile-workspace.spec.ts >> Tasks opens the mobile sidebar with workspace selection without an extra header row
- Location: e2e/mobile-workspace.spec.ts:4:1

# Error details

```
Test timeout of 90000ms exceeded.
```

```
Error: locator.fill: Test timeout of 90000ms exceeded.
Call log:
  - waiting for getByLabel('Name', { exact: true })

```

# Page snapshot

```yaml
- generic [ref=e2]:
  - region "Notifications alt+T"
  - main [ref=e3]:
    - alert [ref=e4]:
      - generic [ref=e11]:
        - heading "Orbit is unavailable" [level=1] [ref=e12]
        - paragraph [ref=e13]: Orbit could not reconnect. It keeps trying, and you can retry now.
      - button "Retry" [active] [ref=e14] [cursor=pointer]
  - generic [ref=e16]:
    - banner [ref=e17]:
      - generic [ref=e18]:
        - link "Home" [ref=e19] [cursor=pointer]:
          - /url: /
        - 'button "Workspace: Gamma" [ref=e20] [cursor=pointer]':
          - generic [ref=e21]: G
          - generic [ref=e22]: Gamma
      - generic [ref=e23]:
        - link "Activity" [ref=e24] [cursor=pointer]:
          - /url: /activity
        - button "Account menu for Test User" [ref=e27] [cursor=pointer]:
          - generic "Test User" [ref=e28]:
            - generic [ref=e29]: TU
            - generic [ref=e30]: Online
        - button "Open sidebar" [ref=e33] [cursor=pointer]
    - alert [ref=e35]:
      - generic [ref=e36]:
        - generic [ref=e37]:
          - generic [ref=e39]: Something went wrong
          - generic [ref=e40]: This part of Orbit stopped working. Reload the page to try again.
        - button "Reload" [ref=e42] [cursor=pointer]
    - navigation "Mobile navigation" [ref=e43]:
      - link [ref=e44] [cursor=pointer]:
        - /url: /
      - link [ref=e49] [cursor=pointer]:
        - /url: /tasks
      - link [ref=e55] [cursor=pointer]:
        - /url: /docs
      - link [ref=e60] [cursor=pointer]:
        - /url: /chat
      - button "Mail" [disabled] [ref=e64]
      - link [ref=e66] [cursor=pointer]:
        - /url: /settings
```

# Test source

```ts
  1  | import { expect, test } from '@playwright/test'
  2  | import { testWorkspace } from '../src/test/workspace'
  3  | 
  4  | test('Tasks opens the mobile sidebar with workspace selection without an extra header row', async ({ page }) => {
  5  |   await page.setViewportSize({ width: 390, height: 844 })
  6  |   const workspaces = [
  7  |     testWorkspace('owner', { id: 'alpha', name: 'Alpha' }),
  8  |     testWorkspace('owner', { id: 'beta', name: 'Beta' }),
  9  |   ]
  10 |   await page.route('**/api/v1/**', async (route) => {
  11 |     const path = new URL(route.request().url()).pathname
  12 |     // the shell's favorites sidebar lists saved views on every page
  13 |     if (/\/views\/[^/]+\/favorite$|\/view-favorites\/order$/.test(path)) { await route.fulfill({ status: 204 }); return }
  14 |     let body: unknown = { items: [], next_cursor: null }
  15 |     if (path.endsWith('/views')) body = []
  16 |     if (path === '/api/v1/setup/status') body = { complete: true }
  17 |     if (path === '/api/v1/auth/me') body = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
  18 |     if (path === '/api/v1/workspaces') body = workspaces
  19 |     if (path === '/api/v1/workspaces' && route.request().method() === 'POST') {
  20 |       expect(route.request().postDataJSON()).toEqual({ name: 'Gamma' })
  21 |       const created = testWorkspace('owner', { id: 'gamma', name: 'Gamma' })
  22 |       workspaces.push(created)
  23 |       body = created
  24 |     }
  25 |     if (path === '/api/v1/workspaces/gamma' && route.request().method() === 'PATCH') {
  26 |       expect(route.request().postDataJSON()).toEqual({ name: 'Gamma renamed', expected_version: 1 })
  27 |       body = testWorkspace('owner', { id: 'gamma', name: 'Gamma renamed', version: 2 })
  28 |     }
  29 |     await route.fulfill({ json: body })
  30 |   })
  31 |   await page.goto('/tasks?workspace=alpha')
  32 |   const menu = page.getByRole('button', { name: 'Open sidebar', exact: true })
  33 |   await expect(menu).toBeVisible()
  34 |   const topbar = page.locator('header').filter({ has: menu })
  35 |   await topbar.getByRole('button', { name: 'Workspace: Alpha' }).click()
  36 |   await page.getByRole('button', { name: 'Beta', exact: true }).click()
  37 |   await expect(page).toHaveURL(/workspace=beta/)
  38 |   await menu.click()
  39 |   const drawer = page.getByRole('dialog', { name: 'Navigation' })
  40 |   await expect(drawer.getByRole('button', { name: /^Workspace:/ })).toHaveCount(0)
  41 |   await expect(drawer.getByRole('button', { name: /^Account menu/ })).toHaveCount(0)
  42 |   await page.keyboard.press('Escape')
  43 |   await expect(drawer).not.toBeVisible()
  44 |   await page.getByRole('link', { name: 'Settings', exact: true }).click()
  45 |   await topbar.getByRole('button', { name: 'Workspace: Beta' }).click()
  46 |   await page.getByRole('button', { name: 'Create workspace', exact: true }).click()
  47 |   await page.getByLabel('New workspace').fill('Gamma')
  48 |   await page.getByRole('button', { name: 'Create workspace', exact: true }).click()
  49 |   await expect(page).toHaveURL(/workspace=gamma/)
  50 |   await expect(page.getByLabel('New workspace')).toHaveCount(0)
  51 |   await expect(page.getByRole('button', { name: 'Create workspace', exact: true })).toHaveCount(0)
  52 |   await expect(page.getByRole('button', { name: 'Save workspace' })).toHaveCount(0)
  53 |   await expect(page.getByRole('button', { name: 'Save Changes' })).toHaveCount(0)
> 54 |   await page.getByLabel('Name', { exact: true }).fill('Draft name')
     |                                                  ^ Error: locator.fill: Test timeout of 90000ms exceeded.
  55 |   await expect(page.getByRole('button', { name: 'Save Changes' })).toBeVisible()
  56 |   const popup = page.locator('.unsaved-bar')
  57 |   await expect.poll(async () => Math.round((await popup.boundingBox())?.y ?? -1)).toBe(12)
  58 |   await page.evaluate(() => {
  59 |     Object.defineProperty(window.visualViewport, 'height', { configurable: true, value: 440 })
  60 |     Object.defineProperty(window.visualViewport, 'offsetTop', { configurable: true, value: 180 })
  61 |     Object.defineProperty(window.visualViewport, 'offsetLeft', { configurable: true, value: 20 })
  62 |     Object.defineProperty(window.visualViewport, 'width', { configurable: true, value: 350 })
  63 |     window.visualViewport!.dispatchEvent(new Event('resize'))
  64 |   })
  65 |   await page.locator('.settings-scroll').evaluate((element) => { element.scrollTop = 240 })
  66 |   await expect.poll(async () => Math.round((await popup.boundingBox())?.y ?? -1)).toBe(192)
  67 |   await expect.poll(async () => Math.round((await popup.boundingBox())?.x ?? -1)).toBe(32)
  68 |   await expect.poll(async () => Math.round((await popup.boundingBox())?.width ?? -1)).toBe(326)
  69 |   await page.evaluate(() => {
  70 |     Object.defineProperty(window.visualViewport, 'offsetTop', { configurable: true, value: 240 })
  71 |     window.visualViewport!.dispatchEvent(new Event('scroll'))
  72 |   })
  73 |   await expect.poll(async () => Math.round((await popup.boundingBox())?.y ?? -1)).toBe(252)
  74 |   await page.evaluate(() => {
  75 |     delete (window.visualViewport as unknown as { height?: number }).height
  76 |     delete (window.visualViewport as unknown as { offsetTop?: number }).offsetTop
  77 |     delete (window.visualViewport as unknown as { offsetLeft?: number }).offsetLeft
  78 |     delete (window.visualViewport as unknown as { width?: number }).width
  79 |     window.visualViewport!.dispatchEvent(new Event('resize'))
  80 |   })
  81 |   await page.setViewportSize({ width: 1280, height: 844 })
  82 |   await expect(popup).toHaveCSS('position', 'absolute')
  83 |   await expect(popup).toHaveCSS('bottom', '20px')
  84 |   await expect.poll(async () => (await popup.boundingBox())!.y).toBeGreaterThan(600)
  85 |   await page.setViewportSize({ width: 390, height: 844 })
  86 |   await expect.poll(async () => Math.round((await popup.boundingBox())?.y ?? -1)).toBe(12)
  87 | 
  88 |   await page.getByRole('button', { name: 'Reset', exact: true }).click()
  89 |   await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Gamma')
  90 |   await expect(page.getByRole('button', { name: 'Save Changes' })).toHaveCount(0)
  91 |   await page.getByLabel('Name', { exact: true }).fill(' Gamma renamed ')
  92 |   await page.getByRole('button', { name: 'Save Changes' }).click()
  93 |   await expect(page.getByRole('button', { name: 'Save Changes' })).toHaveCount(0)
  94 |   await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Gamma renamed')
  95 |   await page.setViewportSize({ width: 1280, height: 844 })
  96 |   await expect(page.locator('.app-sidebar').getByRole('button', { name: 'Workspace: Gamma renamed' })).toBeVisible()
  97 | })
  98 | 
```