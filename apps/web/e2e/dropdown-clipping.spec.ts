import { expect, test } from '@playwright/test'

for (const viewport of [{ width: 1280, height: 800 }, { width: 390, height: 440 }]) {
  test(`dropdown escapes its clipped container at ${viewport.width}x${viewport.height}`, async ({ page }) => {
    await page.setViewportSize(viewport)
    await page.goto('/e2e/fixtures/dropdown.html')
    await page.getByRole('button', { name: 'Actions', exact: true }).click()
    const panel = page.locator('.popover')
    await expect(panel).toBeVisible()
    for (const name of ['10', '25', '50', '100']) {
      // Trial clicks check hit targets without selecting and closing the menu.
      await page.getByRole('button', { name, exact: true }).click({ trial: true })
    }
    const bounds = await panel.boundingBox()
    expect(bounds!.x).toBeGreaterThanOrEqual(0)
    expect(bounds!.y).toBeGreaterThanOrEqual(0)
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(viewport.width)
    expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(viewport.height)
    await page.getByRole('button', { name: '100', exact: true }).click()
    await expect(panel).toHaveCount(0)
  })
}

test('a constrained menu keeps its scroll position and repositions on resize', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 200 })
  await page.goto('/e2e/fixtures/dropdown.html')
  await page.getByRole('button', { name: 'Actions', exact: true }).click()
  const panel = page.locator('.popover')
  await expect(panel).toBeVisible()
  await panel.evaluate((element) => { element.scrollTop = element.scrollHeight })
  // Allow captured scroll handlers to run before checking the lower option.
  await page.waitForTimeout(150)
  expect(await panel.evaluate((element) => element.scrollTop)).toBeGreaterThan(0)
  await page.getByRole('button', { name: '100', exact: true }).click({ trial: true })
  await page.setViewportSize({ width: 390, height: 440 })
  await page.getByRole('button', { name: '10', exact: true }).click({ trial: true })
  await page.keyboard.press('Escape')
  await expect(panel).toHaveCount(0)
})
