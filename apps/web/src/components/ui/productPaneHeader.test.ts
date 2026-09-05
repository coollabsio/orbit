import { expect, test } from 'bun:test'

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
