import { expect, test } from '@playwright/test'

for (const mode of ['new', 'existing', 'wrong-account']) {
  test(`invitation acceptance for ${mode} users keeps the invited email fixed`, async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    let session: { id: string; email: string; display_name: string } | null = mode === 'wrong-account'
      ? { id: 'other', email: 'other@example.com', display_name: 'Other' } : null
    let accepted = false
    await page.route('**/api/v1/**', async (route) => {
      const path = new URL(route.request().url()).pathname
      if (path.endsWith('/invitations/preview')) {
        expect(route.request().postDataJSON()).toEqual({ token: 'invitation-secret' })
        await route.fulfill({ json: { email: 'invited@example.com', workspace_name: 'Alpha' } })
        return
      }
      if (path.endsWith('/auth/me')) {
        await route.fulfill(session ? { json: session } : { status: 401, json: { type: 'about:blank', title: 'Sign in required', status: 401, code: 'authentication_required', detail: 'Sign in required', instance: path, request_id: 'test' } })
        return
      }
      if (path.endsWith('/auth/logout')) {
        session = null
        await route.fulfill({ status: 204 })
        return
      }
      if (path.endsWith('/auth/login')) {
        expect(route.request().postDataJSON().email).toBe('invited@example.com')
        session = { id: 'invited', email: 'invited@example.com', display_name: 'Invited' }
        await route.fulfill({ json: { user: session } })
        return
      }
      if (path.endsWith('/invitations/accept')) {
        const body = route.request().postDataJSON()
        expect(body.token).toBe('invitation-secret')
        if (mode === 'new') {
          expect(body.email).toBe('invited@example.com')
          expect(body.display_name).toBe('Invited')
        } else expect(body).toEqual({ token: 'invitation-secret' })
        accepted = true
        session = { id: 'invited', email: 'invited@example.com', display_name: 'Invited' }
        await route.fulfill({ json: { workspace_id: 'alpha', membership_id: 'membership', created: true } })
        return
      }
      let body: unknown = { items: [], next_cursor: null }
      if (path.endsWith('/setup/status')) body = { complete: true }
      if (path === '/api/v1/workspaces') body = [{ id: 'alpha', name: 'Alpha', role: 'member', version: 1 }]
      await route.fulfill({ json: body })
    })
    await page.goto('/accept-invitation?token=invitation-secret')
    const email = page.getByLabel('Email', { exact: true })
    await expect(email).toHaveValue('invited@example.com')
    await expect(email).toHaveAttribute('readonly', '')
    await expect(page).toHaveURL(/\/accept-invitation$/)
    if (mode === 'wrong-account') {
      await expect(page.getByText(/You are signed in as other@example.com/)).toBeVisible()
      await page.getByRole('button', { name: 'Switch account' }).click()
    } else if (mode === 'existing') {
      await page.getByRole('button', { name: 'Sign in instead' }).click()
    } else {
      await page.getByLabel('Your name').fill('Invited')
    }
    await page.getByLabel('Password', { exact: true }).fill('secure-password')
    await page.getByRole('button', { name: mode === 'new' ? 'Accept invitation' : 'Sign in and accept invitation', exact: true }).click()
    await expect(page).toHaveURL(/\/tasks/)
    expect(accepted).toBe(true)
    await expect(page.getByRole('button', { name: 'Menu', exact: true })).toBeVisible()
  })
}
