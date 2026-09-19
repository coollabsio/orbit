import { expect, test } from 'bun:test'

test('the collapse toggle lives in the footer beside the user menu, not the brand row', async () => {
  const source = await Bun.file(new URL('./AppShell.tsx', import.meta.url)).text()
  const desktopSidebar = source.slice(source.indexOf('<aside className="app-sidebar"'), source.indexOf('</aside>'))

  expect(desktopSidebar).toContain('<SidebarBrand collapsed={sidebarCollapsed} />')
  expect(desktopSidebar).not.toContain('onToggleCollapse')
  const footer = desktopSidebar.slice(desktopSidebar.indexOf('<div className="app-sidebar-footer">'))
  expect(footer).toContain('app-sidebar-collapse')
  expect(footer).toContain('Collapse sidebar')
})

test('the toggle is always visible, not revealed on hover', async () => {
  const css = await Bun.file(new URL('./shell.css', import.meta.url)).text()
  const toggle = css.slice(css.indexOf('.app-sidebar-collapse {'), css.indexOf('.app-sidebar-title {'))

  expect(toggle).not.toContain('opacity: 0;')
  expect(toggle).not.toContain('pointer-events: none;')
  expect(toggle).not.toContain(':hover .app-sidebar-collapse')
  // The collapsed rail still flips the arrow to read as "expand".
  expect(toggle).toMatch(/\.app-sidebar\[data-collapsed='true'\] \.app-sidebar-collapse svg \{\s*transform: rotate\(180deg\);/)
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

test('the UI conventions describe the footer collapse placement and the shared topbar', async () => {
  const conventions = await Bun.file(new URL('../../../../../.ai/UI_CONVENTIONS.md', import.meta.url)).text()

  expect(conventions).toContain('Collapse control sits in the footer')
  expect(conventions).toContain('`[`')
  expect(conventions).not.toContain('feature-owned headers replace the global topbar')
  expect(conventions).toContain('One 48px topbar renders on every route at every width')
})
