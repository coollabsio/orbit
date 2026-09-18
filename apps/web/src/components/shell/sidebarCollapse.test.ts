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
