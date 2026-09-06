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

test('settings sidebar menu stays left of the title without a duplicate gear icon', async () => {
  const [css, topbar] = await Promise.all([
    Bun.file(new URL('../shell/shell.css', import.meta.url)).text(),
    Bun.file(new URL('../shell/Topbar.tsx', import.meta.url)).text(),
  ])
  expect(css).not.toMatch(/\.topbar\[data-root='settings'\] \.topbar-drawer-button\s*\{\s*order: 1;/)
  expect(topbar.indexOf('aria-label="Menu"')).toBeLessThan(topbar.indexOf('<nav className="topbar-crumbs">'))
  const crumbs = topbar.slice(topbar.indexOf('<nav className="topbar-crumbs">'), topbar.indexOf('</nav>'))
  expect(crumbs).not.toContain('<Setting2')
})

test('settings header uses Tasks muted title and toolbar styling', async () => {
  const [css, topbar, settings] = await Promise.all([
    Bun.file(new URL('../shell/shell.css', import.meta.url)).text(),
    Bun.file(new URL('../shell/Topbar.tsx', import.meta.url)).text(),
    Bun.file(new URL('../../features/settings/settings.css', import.meta.url)).text(),
  ])
  expect(css).toMatch(/\.topbar\[data-root='settings'\] \.topbar-crumb\s*\{\s*color: var\(--text-muted\)/)
  expect(css).toMatch(/\.topbar\[data-root='settings'\] \.icon-button\s*\{[^}]*color: var\(--text-muted\)/)
  expect(topbar).toContain("<SearchNormal size={routeRoot === 'settings' ? 15 : 16} />")
  expect(settings).toContain('.settings-page-header .pane-title')
  expect(settings).toContain('color: var(--text-muted)')
})

test('mobile Settings has the same 7px icon-to-label gap as Tasks', async () => {
  const css = await Bun.file(new URL('../shell/shell.css', import.meta.url)).text()
  expect(css).toMatch(/\.topbar\[data-root='settings'\]\s*\{[^}]*gap: 7px;/)
  expect(css).toMatch(/\.topbar\[data-root='settings'\] \.topbar-drawer-button\s*\{[^}]*padding-inline: 7px 0;[^}]*border: 0;/)
})

test('Settings excludes the unrelated New dropdown', async () => {
  const topbar = await Bun.file(new URL('../shell/Topbar.tsx', import.meta.url)).text()
  expect(topbar).toContain("routeRoot !== 'home' && routeRoot !== 'settings' ? <Dropdown")
})
