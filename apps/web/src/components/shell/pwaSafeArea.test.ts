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

test('the app shell fills the webview and keeps chrome inside the safe area', async () => {
  const css = await Bun.file(new URL('./shell.css', import.meta.url)).text()
  const shell = css.slice(css.indexOf('.app-shell {'), css.indexOf('.app-sidebar {'))
  expect(shell).toContain('height: var(--app-height, 100svh)')
  expect(shell).toContain('env(safe-area-inset-top')
  expect(shell).not.toMatch(/height:\s*100dvh/)
  const dock = css.slice(css.indexOf('.mobile-dock {'), css.indexOf('.mobile-dock > a'))
  expect(dock).toContain('env(safe-area-inset-bottom')
  expect(dock).toContain('min-height: var(--dock-height)')
  expect(dock).toContain('padding-top: 12px')
})

test('mobile chrome has no outer or dock hairlines', async () => {
  const css = await Bun.file(new URL('./shell.css', import.meta.url)).text()
  const mobile = css.slice(css.indexOf('@media (max-width: 899px)'))
  expect(mobile).toMatch(/\.mobile-dock\s*\{[^}]*border-top:\s*none/)
  expect(mobile).toMatch(/\.topbar\s*\{[^}]*border-bottom:\s*none/)
  expect(mobile).toMatch(/\.app-shell\s*\{[^}]*border:\s*none/)
})
