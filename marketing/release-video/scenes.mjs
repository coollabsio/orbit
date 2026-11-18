// One function per screenshot. Add the scenes for a new release here, and remove the ones that
// are no longer news. A scene calls shot(name) one time; `name` is the scene's key.
// The v0.4.0 scenes need the demo data of seed-demo.sh and a server started with ORBIT_OIDC_*
// (for the login page's OpenID Connect button).
const dismissDonation = (page) => page.getByRole('button', { name: 'Maybe next time' }).click({ timeout: 2000 }).catch(() => {})

const route = (path, prepare) => async ({ page, shot, base, name }) => {
  await page.goto(base + path)
  await page.waitForTimeout(1500)
  await dismissDonation(page)
  if (prepare) await prepare(page)
  await shot(name)
}

// The id of a project of the first workspace, read with the page's session.
const projectId = (page, projectName) =>
  page.evaluate(async (projectName) => {
    const workspaces = await (await fetch('/api/v1/workspaces')).json()
    const workspace = (workspaces[0] ?? workspaces.workspaces[0]).id
    const projects = await (await fetch(`/api/v1/workspaces/${workspace}/projects`)).json()
    return projects.items.find((project) => project.name === projectName).id
  }, projectName)

const projectSettings = (projectName, section, prepare) => async ({ page, shot, base, name }) => {
  await page.goto(base + '/tasks')
  await dismissDonation(page)
  await page.goto(`${base}/tasks/projects/${await projectId(page, projectName)}/settings/${section}`)
  await page.waitForTimeout(1500)
  if (prepare) await prepare(page)
  await shot(name)
}

export const scenes = {
  // v0.4.0
  'mail': route('/mail', async (page) => {
    await page.getByText('Can we move our workspace').click()
    await page.waitForTimeout(1000)
  }),
  'milestones': async ({ page, shot, base, name }) => {
    await page.goto(base + '/tasks')
    await dismissDonation(page)
    await page.goto(`${base}/tasks?project=${await projectId(page, 'Orbit Web')}`)
    await page.waitForTimeout(1500)
    await page.getByRole('button', { name: 'Display' }).click()
    await page.getByText('Milestone timeline').click()
    await page.keyboard.press('Escape')
    await shot(name, { wait: 1500 })
  },
  'cycles': projectSettings('Orbit Web', 'cycles'),
  'recurring': route('/tasks/WEB-11', (page) => page.getByRole('button', { name: 'Repeat', exact: true }).click()),
  'private-project': projectSettings('Security review', 'access'),
  'roles': route('/settings/roles'),
  'oidc-login': async ({ page, shot, base, name }) => {
    const cookies = await page.context().cookies()
    await page.context().clearCookies()
    await page.goto(base + '/login')
    await page.waitForTimeout(1500)
    await shot(name)
    await page.context().addCookies(cookies)
  },
  'search': route('/tasks', async (page) => {
    await page.keyboard.press('Control+k')
    await page.waitForTimeout(400)
    await page.keyboard.type('beta', { delay: 60 })
    await page.waitForTimeout(1200)
  }),
  'task-refs': route('/chat', async (page) => {
    await page.getByRole('textbox').last().click()
    await page.keyboard.type('Can you pick up ', { delay: 20 })
    await page.keyboard.type('#beta', { delay: 120 })
    await page.waitForTimeout(1200)
  }),
  'notifications': route('/tasks', async (page) => {
    await page.getByRole('button', { name: /^Activity/ }).click()
    await page.waitForTimeout(1200)
  }),
  'appearance': route('/profile/appearance'),
  'font-size': route('/profile/appearance', async (page) => {
    await page.getByText('Text size').first().scrollIntoViewIfNeeded()
    await page.mouse.wheel(0, 300)
    await page.waitForTimeout(500)
  }),
  'webhooks': route('/settings/webhooks'),
}
