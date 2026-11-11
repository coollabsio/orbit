// One function per screenshot. Add the scenes for a new release here, and remove the ones that
// are no longer news. A scene calls shot(name) one time; `name` is the scene's key.
const dismissDonation = (page) => page.getByRole('button', { name: 'Maybe next time' }).click({ timeout: 2000 }).catch(() => {})

async function openDemoDatabase(page, base) {
  await page.goto(base + '/docs')
  await page.getByText('Launch plan').first().click()
  await page.waitForTimeout(1500)
}

// The database opens on the view that was used last, so a scene always picks its view.
const pickView = (page, label) =>
  page.getByRole('tab', { name: label }).or(page.getByRole('button', { name: label, exact: true })).first().click()

const view = (label) => async ({ page, shot, base, name }) => {
  await openDemoDatabase(page, base)
  await dismissDonation(page)
  await pickView(page, label)
  await shot(name, { wait: 1500 })
}

const route = (path, prepare) => async ({ page, shot, base, name }) => {
  await page.goto(base + path)
  await page.waitForTimeout(1500)
  await dismissDonation(page)
  if (prepare) await prepare(page)
  await shot(name)
}

export const scenes = {
  // v0.3.0
  'donation-reminder': async ({ page, shot, base, name }) => {
    await openDemoDatabase(page, base) // the reminder shows on the first page after sign-in
    await shot(name)
  },
  'db-table': view('Table'),
  'db-board': view('Board'),
  'db-calendar': view('Calendar'),
  'db-timeline': view('Timeline'),
  'db-gallery': view('Gallery'),
  'db-list': view('List'),
  'db-form': view('Feedback form'),
  'db-peek': async ({ page, shot, base, name }) => {
    await openDemoDatabase(page, base)
    await dismissDonation(page)
    await pickView(page, 'Table')
    await page.getByText('Launch Orbit Cloud EU region').first().hover()
    await page.getByRole('button', { name: 'Open' }).first().click()
    await shot(name, { wait: 1500 })
  },
  'import': route('/settings/import'),
  'import-orbit': route('/settings/import/orbit'),
  'members-guest': route('/settings/members', (page) => page.getByRole('combobox').last().click()),
  'docs-trash': route('/docs/trash'),
}
