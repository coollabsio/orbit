import { expect, test } from 'bun:test'

test('the collapse toggle lives in the brand row, not the footer', async () => {
  const source = await Bun.file(new URL('./AppShell.tsx', import.meta.url)).text()
  const desktopSidebar = source.slice(source.indexOf('<aside className="app-sidebar"'), source.indexOf('</aside>'))

  expect(desktopSidebar).toContain('<SidebarBrand collapsed={sidebarCollapsed} onToggleCollapse={toggleSidebar} />')
  const footer = desktopSidebar.slice(desktopSidebar.indexOf('<div className="app-sidebar-footer">'))
  expect(footer).not.toContain('app-sidebar-collapse')
  expect(footer).not.toContain('Collapse sidebar')
})

test('the mobile drawer brand row never gains a collapse control', async () => {
  const source = await Bun.file(new URL('./AppShell.tsx', import.meta.url)).text()
  const drawer = source.slice(source.indexOf('<aside className="mobile-drawer">'))

  expect(drawer).toContain('<SidebarBrand onSelectWorkspace={() => setDrawerOpen(false)} />')
  expect(drawer).not.toContain('onToggleCollapse')
})

test('collapsed state is still keyboard reachable and still persisted under the same key', async () => {
  const source = await Bun.file(new URL('./AppShell.tsx', import.meta.url)).text()

  expect(source).toContain('useSidebarToggleShortcut(toggleSidebar)')
  expect(source).toContain('const toggleSidebar = useCallback(() => setSidebarCollapsed((collapsed) => !collapsed), [])')
  expect(source).toContain("window.localStorage.getItem('orbit:sidebar_collapsed')")
  expect(source).toContain("window.localStorage.setItem('orbit:sidebar_collapsed', String(sidebarCollapsed))")
})

test('the UI conventions describe the new collapse placement and the shared topbar', async () => {
  const conventions = await Bun.file(new URL('../../../../../.ai/UI_CONVENTIONS.md', import.meta.url)).text()

  expect(conventions).not.toContain('Collapse control is at the bottom beside the user menu.')
  expect(conventions).toContain('Collapse control sits in the 48px brand row')
  expect(conventions).toContain('`[`')
  expect(conventions).not.toContain('feature-owned headers replace the global topbar')
  expect(conventions).toContain('One 48px topbar renders on every route at every width')
})
