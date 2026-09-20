import { expect, test } from 'bun:test'

test('mobile settings keeps shell actions and hides only the duplicate page header', async () => {
  const topbar = await Bun.file(new URL('./Topbar.tsx', import.meta.url)).text()
  // the tasks top bar is hidden on mobile, but the settings top bar is kept (its shell actions stay)
  expect(topbar).toContain('data-[root=tasks]:hidden')
  expect(topbar).not.toMatch(/data-\[root=settings\]:hidden/)
  // Settings is Tailwind now: the duplicate page header hides on mobile so the shell topbar takes over.
  const settings = await Bun.file(new URL('../../features/settings/pages/SettingsLayout.tsx', import.meta.url)).text()
  expect(settings).toContain('max-[899px]:hidden')
})

test('tasks and settings share the mobile product header contract', async () => {
  const [tasks, settings] = await Promise.all([
    Bun.file(new URL('../../features/tasks/pages/TasksPage.tsx', import.meta.url)).text(),
    Bun.file(new URL('../../features/settings/pages/SettingsLayout.tsx', import.meta.url)).text(),
  ])
  // Tasks is Tailwind now: its product header collapses to the 44px mobile height (min-h-11).
  expect(tasks).toContain('max-[899px]:min-h-11')
  // Settings is migrated to Tailwind: its duplicate page header hides on mobile so the shell topbar takes over.
  expect(settings).toContain('max-[899px]:hidden')
})

test('inbox owns a mobile menu button while its shell topbar is hidden', async () => {
  const [topbar, inbox] = await Promise.all([
    Bun.file(new URL('./Topbar.tsx', import.meta.url)).text(),
    Bun.file(new URL('../../features/inbox/pages/InboxPage.tsx', import.meta.url)).text(),
  ])
  expect(topbar).toContain('data-[root=inbox]:hidden')
  expect(inbox).toContain('max-[899px]:inline-flex')
  expect(inbox).toContain('aria-label="Menu"')
  expect(inbox).toContain("new CustomEvent('open-sidebar')")
})

test('settings sidebar menu stays left of the title without a duplicate gear icon', async () => {
  const topbar = await Bun.file(new URL('./Topbar.tsx', import.meta.url)).text()
  // the menu (drawer) button renders before the breadcrumb nav
  expect(topbar.indexOf('aria-label="Menu"')).toBeLessThan(topbar.indexOf('<nav '))
  const crumbs = topbar.slice(topbar.indexOf('<nav '), topbar.indexOf('</nav>'))
  expect(crumbs).not.toContain('<Settings')
})

test('settings header uses Tasks muted title and toolbar styling', async () => {
  const [topbar, settings] = await Promise.all([
    Bun.file(new URL('./Topbar.tsx', import.meta.url)).text(),
    Bun.file(new URL('../../features/settings/pages/SettingsLayout.tsx', import.meta.url)).text(),
  ])
  // home lifts the crumb to the heading color; settings keeps the muted base color
  expect(topbar).toMatch(/group-data-\[root=home\]\/topbar:text-foreground/)
  expect(topbar).not.toMatch(/group-data-\[root=settings\]\/topbar:text-foreground/)
  // the settings search icon is the smaller 15px size, tasks uses 16px (size-4)
  expect(topbar).toContain("routeRoot === 'settings' ? 'size-[15px]' : 'size-4'")
  // the settings page header keeps the muted title/icon color (Tailwind token)
  expect(settings).toContain('text-muted-foreground')
})

test('settings menu button matches the tasks burger size', async () => {
  const topbar = await Bun.file(new URL('./Topbar.tsx', import.meta.url)).text()
  // the burger icon is 18px regardless of route, and the button uses the shared 28px icon-sm size
  expect(topbar).toContain('<Menu className="size-[18px]" />')
  expect(topbar).not.toContain("? 15 : 18")
  expect(topbar).toContain('size="icon-sm"')
})

test('Settings and Profile exclude the unrelated New dropdown', async () => {
  const topbar = await Bun.file(new URL('./Topbar.tsx', import.meta.url)).text()
  expect(topbar).toContain("routeRoot !== 'home' && routeRoot !== 'settings' && routeRoot !== 'profile'")
  expect(topbar).toContain('<DropdownMenu>')
})
