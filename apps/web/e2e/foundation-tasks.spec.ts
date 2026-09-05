import { expect, test, type Page } from '@playwright/test'

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

async function createWorkspaceAndInviteMember(page: Page) {
  await page.goto('/settings/members')
  await page.getByLabel('Email address').fill('member@orbit.test')
  await page.getByRole('button', { name: 'Generate link' }).click()
  await expect(page.getByLabel(/Invitation link/)).toHaveValue(/\/accept-invitation\?token=/)
  await page.getByRole('button', { name: 'Copy' }).click()
  await expect(page.getByRole('button', { name: 'Copied' })).toBeVisible()
}

async function createTaskWithAttachment(page: Page) {
  await page.goto('/tasks')
  await page.getByRole('button', { name: 'New task' }).click()
  await expect(page).toHaveURL(/\/tasks\/.+/)
  const title = page.getByLabel('Task title')
  await expect(title).toBeFocused()
  await title.fill('Restored task')
  await title.press('Tab')
  await page.getByLabel('Attach files').setInputFiles({
    name: 'proof.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('persisted attachment'),
  })
  await expect(page.getByText('proof.txt')).toBeVisible()
}

async function deleteAndRestoreTask(page: Page) {
  page.once('dialog', (dialog) => dialog.accept())
  await page.getByRole('button', { name: 'Delete task' }).click()
  await expect(page).toHaveURL(/\/tasks(?:\?|$)/)
  await page.goto('/tasks-trash')
  await page.getByRole('button', { name: 'Restore' }).click()
  await expect(page.getByText('Restored task')).not.toBeVisible()
}

test('setup through restored task', async ({ page }) => {
  await bootstrapOwner(page)
  await createWorkspaceAndInviteMember(page)
  await createTaskWithAttachment(page)
  await deleteAndRestoreTask(page)
  await page.goto('/tasks')
  await expect(page.getByText('Restored task')).toBeVisible()
})
