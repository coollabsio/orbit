import { expect, test, type Browser, type Page } from '@playwright/test'
import { CONTRACT_ID } from '../src/api/generated/contract'

async function api<T>(page: Page, path: string, method = 'GET', body?: unknown): Promise<T> {
  return page.evaluate(async ({ path, method, body, contract }) => {
    const response = await fetch(path, {
      method,
      headers: { 'Content-Type': 'application/json', 'X-Orbit-Contract': contract },
      body: body === undefined ? undefined : JSON.stringify(body),
    })
    if (!response.ok) throw new Error(`${method} ${path}: ${response.status}`)
    return response.status === 204 ? undefined : response.json()
  }, { path, method, body, contract: CONTRACT_ID }) as Promise<T>
}

async function bootstrapOwner(page: Page) {
  const setupUrl = process.env.ORBIT_SETUP_URL
  if (!setupUrl) throw new Error('ORBIT_SETUP_URL must be provided by just e2e')
  await page.goto(setupUrl)
  await page.getByLabel('Your name').fill('Orbit Owner')
  await page.getByLabel('Email').fill('owner@orbit.test')
  await page.getByLabel('Password').fill('correct horse battery staple')
  await page.getByLabel('Workspace name').fill('Foundation')
  await page.getByLabel('First project').fill('Launch')
  await page.getByRole('button', { name: 'Create workspace' }).click()
  await expect(page).toHaveURL(/\/($|\?)/)

  await page.getByRole('button', { name: 'Account menu for Orbit Owner' }).click()
  await page.getByRole('button', { name: 'Log out' }).click()
  await expect(page).toHaveURL(/\/login$/)
  await page.getByLabel('Email').fill('owner@orbit.test')
  await page.getByLabel('Password').fill('correct horse battery staple')
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page).toHaveURL(/\/($|\?)/)
}

async function createWorkspaceAndInviteMember(page: Page, browser: Browser) {
  await page.goto('/settings/members')
  await page.getByLabel('Email address').fill('member@orbit.test')
  await page.getByRole('button', { name: 'Generate link' }).click()
  await expect(page.getByLabel(/Invitation link/)).toHaveValue(/\/accept-invitation\?token=/)
  await page.getByRole('button', { name: 'Copy' }).click()
  await expect(page.getByRole('button', { name: 'Copied' })).toBeVisible()
  const invitation = await page.getByLabel(/Invitation link/).inputValue()

  const inviteeContext = await browser.newContext()
  const invitee = await inviteeContext.newPage()
  await invitee.goto(invitation)
  await invitee.getByLabel('Name (new accounts)').fill('Workspace Member')
  await invitee.getByLabel('Email (new accounts)').fill('member@orbit.test')
  await invitee.getByLabel('Password (new accounts)').fill('member password is long enough')
  await invitee.getByRole('button', { name: 'Accept invitation' }).click()
  await expect(invitee.getByRole('button', { name: 'Workspace: Foundation' })).toBeVisible()
  await invitee.goto(new URL('/settings/members', invitation).href)
  await expect(invitee.locator('.data-table-row').filter({ hasText: 'member@orbit.test' })).toContainText('Member')
  await inviteeContext.close()

  const newTab = await page.context().newPage()
  await newTab.goto('/settings/sessions')
  await expect(newTab.getByText('Current')).toBeVisible()
  await expect(newTab.getByRole('button', { name: 'Revoke' })).toHaveCount(0)
  await newTab.close()

  await page.goto('/settings')
  await page.getByRole('button', { name: 'Workspace: Foundation' }).click()
  await page.getByRole('button', { name: 'Create workspace' }).click()
  await page.getByLabel('New workspace').fill('Second workspace')
  await page.getByRole('button', { name: 'Create workspace' }).click()
  await expect(page.getByRole('button', { name: 'Workspace: Second workspace' })).toBeVisible()
  await page.getByRole('button', { name: 'Workspace: Second workspace' }).click()
  await page.getByRole('button', { name: 'Foundation' }).click()
  await expect(page.getByRole('button', { name: 'Workspace: Foundation' })).toBeVisible()
}

async function revokeAnotherOwnerSession(page: Page, browser: Browser) {
  const secondContext = await browser.newContext()
  const secondSession = await secondContext.newPage()
  await secondSession.goto('/login')
  await secondSession.getByLabel('Email').fill('owner@orbit.test')
  await secondSession.getByLabel('Password').fill('correct horse battery staple')
  await secondSession.getByRole('button', { name: 'Sign in' }).click()
  await expect(secondSession).toHaveURL(/\/(?:$|\?)/)

  await page.goto('/settings/sessions')
  const revoke = page.getByRole('button', { name: 'Revoke' })
  await expect(revoke).toHaveCount(1)
  await revoke.click()
  await expect(revoke).toHaveCount(0)

  await secondSession.goto('/settings')
  await expect(secondSession).toHaveURL(/\/login$/)
  await secondContext.close()
}

async function createTaskWithAttachment(page: Page) {
  const workspaces = await api<Array<{ id: string; name: string }>>(page, '/api/v1/workspaces')
  const workspaceId = workspaces.find((workspace) => workspace.name === 'Foundation')!.id
  await api(page, `/api/v1/workspaces/${workspaceId}/labels`, 'POST', { name: 'Unused label', color: '#123456' })
  await page.goto('/tasks')
  await page.getByRole('button', { name: 'New task' }).click()
  await expect(page).toHaveURL(/\/tasks\/.+/)
  const title = page.getByLabel('Task title')
  await expect(title).toBeFocused()
  await title.fill('Restored task')
  const titleSaved = page.waitForResponse((response) => response.request().method() === 'PATCH' && /\/tasks\/[^/]+$/.test(new URL(response.url()).pathname))
  await title.press('Tab')
  await titleSaved
  await page.getByLabel('Attach files').setInputFiles({
    name: 'proof.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('persisted attachment'),
  })
  await expect(page.getByText('proof.txt')).toBeVisible()
  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.getByRole('link', { name: /proof\.txt/ }).click(),
  ])
  expect(download.suggestedFilename()).toBe('proof.txt')

  const priority = page.locator('.tasks-side-group').first().getByRole('button').nth(1)
  await priority.click()
  await page.getByRole('button', { name: 'Urgent' }).click()
  await expect(priority).toContainText('Urgent')

  await page.getByRole('button', { name: 'Add label' }).click()
  await page.getByRole('button', { name: 'Unused label' }).click()
  await expect(page.locator('.tasks-label-pill').filter({ hasText: 'Unused label' })).toBeVisible()

  const status = page.locator('.tasks-side-group').first().getByRole('button').first()
  await status.click()
  await page.getByRole('button', { name: 'In Progress' }).click()
  await expect(status).toContainText('In Progress')

  const composer = page.getByPlaceholder('Leave a comment…').locator('..')
  await page.getByPlaceholder('Leave a comment…').fill('Persisted comment')
  await composer.getByRole('button', { name: 'Send' }).click()
  await expect(page.getByText('Persisted comment')).toBeVisible()
  await composer.getByLabel('Attach comment files').setInputFiles({
    name: 'comment-proof.txt', mimeType: 'text/plain', buffer: Buffer.from('attachment-only comment'),
  })
  await composer.getByRole('button', { name: 'Send' }).click()
  await expect(page.getByText('comment-proof.txt')).toBeVisible()

  const taskId = new URL(page.url()).pathname.split('/').at(-1)!
  const record = await api<{ version: number }>(page, `/api/v1/workspaces/${workspaceId}/tasks/${taskId}`)
  await api(page, `/api/v1/workspaces/${workspaceId}/tasks/${taskId}`, 'PATCH', { expected_version: record.version, title: 'Server title' })
  page.once('dialog', (dialog) => dialog.accept())
  await title.fill('Rejected title')
  await title.press('Tab')
  await expect(title).toHaveValue('Server title')
  await title.fill('Restored task')
  await title.press('Tab')
}

async function deleteAndRestoreTask(page: Page) {
  page.once('dialog', (dialog) => dialog.accept())
  await page.getByRole('button', { name: 'Delete task' }).click()
  await expect(page).toHaveURL(/\/tasks(?:\?|$)/)
  await page.goto('/tasks-trash')
  let failRestore = true
  await page.route('**/restore', async (route) => {
    if (!failRestore) return route.continue()
    failRestore = false
    await route.fulfill({
      status: 503,
      contentType: 'application/problem+json',
      body: JSON.stringify({ type: 'about:blank', title: 'Unavailable', status: 503, detail: 'Try again', code: 'unavailable', instance: '/restore', request_id: 'e2e' }),
    })
  })
  await page.getByRole('button', { name: 'Restore' }).click()
  await expect(page.getByRole('alert')).toContainText('Task restore failed')
  await page.getByRole('button', { name: 'Retry' }).click()
  await expect(page.getByText('Restored task')).not.toBeVisible()
  await page.unroute('**/restore')
}

test('setup through restored task', async ({ page, browser }) => {
  await bootstrapOwner(page)
  await createWorkspaceAndInviteMember(page, browser)
  await revokeAnotherOwnerSession(page, browser)
  await createTaskWithAttachment(page)
  await deleteAndRestoreTask(page)
  await page.goto('/tasks')
  await expect(page.getByText('Restored task')).toBeVisible()

  await page.getByRole('button', { name: 'Workspace: Foundation' }).click()
  await page.getByRole('button', { name: 'Second workspace' }).click()
  await expect(page.getByRole('button', { name: 'Workspace: Second workspace' })).toBeVisible()
  await expect(page.getByText('Restored task')).not.toBeVisible()
  await page.getByRole('button', { name: 'Workspace: Second workspace' }).click()
  await page.getByRole('button', { name: 'Foundation' }).click()
  await expect(page.getByRole('button', { name: 'Workspace: Foundation' })).toBeVisible()
  const foundationSearch = new URL(page.url()).search

  await page.goto(`/docs${foundationSearch}`)
  await expect(page.getByText('Mock data')).toBeVisible()
  await page.goto(`/tasks${foundationSearch}`)
  await expect(page.getByText('Mock data')).not.toBeVisible()

  await page.goto(`/settings/members${foundationSearch}`)
  const memberRow = page.locator('.data-table-row').filter({ hasText: 'member@orbit.test' })
  await memberRow.getByRole('button', { name: 'Manage' }).click()
  page.once('dialog', (dialog) => dialog.accept())
  await page.getByRole('button', { name: 'Transfer ownership' }).click()
  await expect(page.locator('.data-table-row').filter({ hasText: 'owner@orbit.test' })).toContainText('Admin')
  await expect(page.locator('.data-table-row').filter({ hasText: 'Workspace Member' })).toContainText('Owner')
})
