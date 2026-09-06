import { expect, test } from 'bun:test'

test('mobile settings keeps shell actions and hides only the duplicate page header', async () => {
  const css = await Bun.file(new URL('../shell/shell.css', import.meta.url)).text()
  const mobile = css.slice(css.indexOf('@media (max-width: 899px)'))
  const hiddenHeaders = mobile.match(/\.topbar\[data-root='tasks'\][^{]+\{\s*display: none;\s*\}/)?.[0]
  expect(hiddenHeaders).not.toContain(".topbar[data-root='settings']")
  const settingsCss = await Bun.file(new URL('../../features/settings/settings.css', import.meta.url)).text()
  expect(settingsCss).toMatch(/@media \(max-width: 899px\)\s*\{\s*\.settings-page-header\s*\{\s*display: none;/)
})

test('tasks and settings share the mobile product header contract', async () => {
  const [utilities, tasks, settings] = await Promise.all([
    Bun.file(new URL('../../styles/utilities.css', import.meta.url)).text(),
    Bun.file(new URL('../../features/tasks/TasksPage.tsx', import.meta.url)).text(),
    Bun.file(new URL('../../features/settings/SettingsLayout.tsx', import.meta.url)).text(),
  ])
  expect(tasks).toContain('pane-header product-pane-header')
  expect(settings).toContain('pane-header product-pane-header')
  expect(utilities).toContain('.product-pane-header')
  expect(utilities).toContain('min-height: 44px')
  expect(utilities).toContain('.product-pane-header-icon')
})
