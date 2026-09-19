import { expect, test } from 'bun:test'

test('mobile settings keeps shell actions and hides only the duplicate page header', async () => {
  const topbar = await Bun.file(new URL('../shell/Topbar.tsx', import.meta.url)).text()
  // the tasks top bar is hidden on mobile, but the settings top bar is kept (its shell actions stay)
  expect(topbar).toContain('data-[root=tasks]:hidden')
  expect(topbar).not.toMatch(/data-\[root=settings\]:hidden/)
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

test('settings sidebar menu stays left of the title without a duplicate gear icon', async () => {
  const topbar = await Bun.file(new URL('../shell/Topbar.tsx', import.meta.url)).text()
  // the menu (drawer) button renders before the breadcrumb nav
  expect(topbar.indexOf('aria-label="Menu"')).toBeLessThan(topbar.indexOf('<nav '))
  const crumbs = topbar.slice(topbar.indexOf('<nav '), topbar.indexOf('</nav>'))
  expect(crumbs).not.toContain('<Settings')
})

test('settings header uses Tasks muted title and toolbar styling', async () => {
  const [topbar, settings] = await Promise.all([
    Bun.file(new URL('../shell/Topbar.tsx', import.meta.url)).text(),
    Bun.file(new URL('../../features/settings/settings.css', import.meta.url)).text(),
  ])
  // home lifts the crumb to the heading color; settings keeps the muted base color
  expect(topbar).toMatch(/group-data-\[root=home\]\/topbar:text-foreground/)
  expect(topbar).not.toMatch(/group-data-\[root=settings\]\/topbar:text-foreground/)
  // the settings search icon is the smaller 15px size, tasks uses 16px (size-4)
  expect(topbar).toContain("routeRoot === 'settings' ? 'size-[15px]' : 'size-4'")
  expect(settings).toContain('.settings-page-header .pane-title')
  expect(settings).toContain('color: var(--text-muted)')
})

test('settings menu button matches the tasks burger size', async () => {
  const topbar = await Bun.file(new URL('../shell/Topbar.tsx', import.meta.url)).text()
  // the burger icon is 18px regardless of route, and the button uses the shared 28px icon-sm size
  expect(topbar).toContain('<Menu className="size-[18px]" />')
  expect(topbar).not.toContain("? 15 : 18")
  expect(topbar).toContain('size="icon-sm"')
})

test('Settings and Profile exclude the unrelated New dropdown', async () => {
  const topbar = await Bun.file(new URL('../shell/Topbar.tsx', import.meta.url)).text()
  expect(topbar).toContain("routeRoot !== 'home' && routeRoot !== 'settings' && routeRoot !== 'profile'")
  expect(topbar).toContain("? <Dropdown")
})
