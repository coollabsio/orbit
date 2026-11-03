# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: sidebar-profile.spec.ts >> mobile shell shows the signed-in profile and supports logout retry
- Location: e2e/sidebar-profile.spec.ts:5:3

# Error details

```
Error: expect(locator).toBeVisible() failed

Locator: getByRole('heading', { name: 'Sign in to Orbit' })
Expected: visible
Timeout: 5000ms
Error: element(s) not found

Call log:
  - Expect "toBeVisible" getByRole('heading', { name: 'Sign in to Orbit' }) with timeout 5000ms
  - waiting for getByRole('heading', { name: 'Sign in to Orbit' })

```

```yaml
- region "Notifications alt+T"
- main:
  - heading "Orbit" [level=1]
  - paragraph: Sign in to your tasks, docs, mail and chat.
  - text: Email *
  - textbox "Email *": test@example.com
  - text: Password *
  - group:
    - textbox "Password *": password
    - group:
      - button "Show password":
        - img
  - link "Forgot password?":
    - /url: /recovery
  - button "Sign in"
```

# Test source

```ts
  1  | import { expect, test } from '@playwright/test'
  2  | import { testWorkspace } from '../src/test/workspace'
  3  | 
  4  | for (const mode of ['desktop', 'collapsed', 'mobile']) {
  5  |   test(`${mode} shell shows the signed-in profile and supports logout retry`, async ({ page }) => {
  6  |     await page.setViewportSize({ width: mode === 'mobile' ? 390 : 1280, height: 844 })
  7  |     let signedIn = true
  8  |     let attempts = 0
  9  |     let finishLogout!: () => void
  10 |     await page.route('**/api/v1/**', async (route) => {
  11 |       const path = new URL(route.request().url()).pathname
  12 |       if (path === '/api/v1/auth/logout') {
  13 |         expect(route.request().method()).toBe('POST')
  14 |         attempts++
  15 |         if (attempts === 1) {
  16 |           await route.fulfill({ status: 503, json: { detail: 'Please try again.' } })
  17 |           return
  18 |         }
  19 |         await new Promise<void>((resolve) => { finishLogout = resolve })
  20 |         signedIn = false
  21 |         await route.fulfill({ status: 204 })
  22 |         return
  23 |       }
  24 |       if (path === '/api/v1/auth/me' && !signedIn) {
  25 |         await route.fulfill({ status: 401, json: { code: 'authentication_required' } })
  26 |         return
  27 |       }
  28 |       // the shell's favorites sidebar lists saved views on every page
  29 |       if (/\/views\/[^/]+\/favorite$|\/view-favorites\/order$/.test(path)) { await route.fulfill({ status: 204 }); return }
  30 |       let body: unknown = { items: [], next_cursor: null }
  31 |       if (path.endsWith('/views')) body = []
  32 |       if (path === '/api/v1/setup/status') body = { complete: true }
  33 |       if (path === '/api/v1/auth/me') body = { id: 'user-1', display_name: 'Test User', email: 'test@example.com' }
  34 |       if (path === '/api/v1/workspaces') body = [testWorkspace('owner', { id: 'alpha', name: 'Alpha' })]
  35 |       await route.fulfill({ json: body })
  36 |     })
  37 |     await page.goto('/tasks?workspace=alpha')
  38 |     if (mode === 'collapsed') await page.getByRole('button', { name: 'Collapse sidebar' }).click()
  39 |     const shell = mode === 'mobile' ? page.locator('header').filter({ has: page.getByRole('button', { name: 'Open sidebar', exact: true }) }) : page.locator('aside').first()
  40 |     const trigger = shell.getByRole('button', { name: 'Account menu for Test User' })
  41 |     await expect(trigger).toBeVisible()
  42 |     if (mode === 'desktop') await expect(trigger).toContainText('Test User')
  43 |     if (mode !== 'mobile') expect((await trigger.boundingBox())!.y).toBeGreaterThan(700)
  44 |     await trigger.click()
  45 |     await page.keyboard.press('Escape')
  46 |     await expect(trigger).toHaveAttribute('aria-expanded', 'false')
  47 |     await trigger.click()
  48 |     const logout = page.getByRole('menuitem', { name: 'Log out', exact: true })
  49 |     await expect(logout).toBeVisible()
  50 |     const popup = page.getByRole('menu')
  51 |     await expect(popup).toContainText('test@example.com')
  52 |     expect((await popup.boundingBox())!.width).toBeLessThan(250)
  53 |     await logout.click()
  54 |     await expect(page.getByRole('alert')).toContainText('Could not log out')
  55 |     await expect(page).toHaveURL(/tasks/)
  56 |     await logout.click()
  57 |     await expect(page.getByRole('menuitem', { name: 'Logging out…' })).toHaveAttribute('aria-disabled', 'true')
  58 |     await expect.poll(() => attempts).toBe(2)
  59 |     finishLogout()
  60 |     await expect(page).toHaveURL(/\/login$/)
> 61 |     await expect(page.getByRole('heading', { name: 'Sign in to Orbit' })).toBeVisible()
     |                                                                           ^ Error: expect(locator).toBeVisible() failed
  62 |     expect(attempts).toBe(2)
  63 |     await page.goto('/tasks?workspace=alpha')
  64 |     await expect(page).toHaveURL(/\/login$/)
  65 |   })
  66 | }
  67 | 
```