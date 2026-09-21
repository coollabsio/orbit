import { expect, test } from 'bun:test'

test('standalone web apps opt into iOS safe-area insets', async () => {
  const html = await Bun.file(new URL('../../../index.html', import.meta.url)).text()
  expect(html).toContain('viewport-fit=cover')
  expect(html).toMatch(/apple-mobile-web-app-capable/)
  expect(html).toMatch(/apple-mobile-web-app-status-bar-style/)
  expect(html).toContain('rel="manifest"')
  const manifest = await Bun.file(new URL('../../../public/manifest.webmanifest', import.meta.url)).json()
  expect(manifest.display).toBe('standalone')
  expect(manifest.scope).toBe('/')
})

test('the app shell fills the webview and only pads the dock in standalone mode', async () => {
  const shell = await Bun.file(new URL('./AppShell.tsx', import.meta.url)).text()
  expect(shell).toContain('h-[var(--app-height,100svh)]')
  expect(shell).toContain('pt-[env(safe-area-inset-top,0px)]')
  expect(shell).not.toMatch(/100dvh/)
  const css = await Bun.file(new URL('../../index.css', import.meta.url)).text()
  expect(css).toContain('@media (display-mode: standalone)')
  expect(css).toContain('height: var(--app-height, 100vh)')
  const viewport = await Bun.file(new URL('../../lib/visibleViewport.ts', import.meta.url)).text()
  expect(viewport).toContain('full - visual > KEYBOARD_SHORTFALL')
  const dock = await Bun.file(new URL('./MobileDock.tsx', import.meta.url)).text()
  expect(dock).toContain('min-h-14') // --dock-height (56px)
  expect(dock).toContain('pt-3') // padding-top 12px
  // the dock only extends behind the home indicator when installed (standalone)
  expect(dock).toContain('[@media(display-mode:standalone)]:pb-[env(safe-area-inset-bottom,0px)]')
})

test('mobile chrome has no outer or dock hairlines', async () => {
  // the dock carries no border in any state
  const dock = await Bun.file(new URL('./MobileDock.tsx', import.meta.url)).text()
  expect(dock).not.toMatch(/border/)
  // the mobile top bar is borderless except for the settings sub-header
  const topbar = await Bun.file(new URL('./Topbar.tsx', import.meta.url)).text()
  expect(topbar).not.toMatch(/(?<!settings\]:)border-b\b/)
  // the only outer vertical hairline lives on the sidebar, which is hidden on mobile
  const shell = await Bun.file(new URL('./AppShell.tsx', import.meta.url)).text()
  expect(shell).toMatch(/border-r border-border[^"]*max-\[899px\]:hidden/)
})

test('mobile drawer stays below portalled workspace and profile menus', async () => {
  const shell = await Bun.file(new URL('./AppShell.tsx', import.meta.url)).text()
  const popover = await Bun.file(new URL('../../components/ui/popover.tsx', import.meta.url)).text()
  const menu = await Bun.file(new URL('../../components/ui/dropdown-menu.tsx', import.meta.url)).text()

  expect(shell).toContain('z-40 bg-black/50')
  expect(shell).toContain('z-[41] flex w-[min(280px,84vw)]')
  expect(popover).toContain('isolate z-50')
  expect(menu).toContain('isolate z-50')
})
