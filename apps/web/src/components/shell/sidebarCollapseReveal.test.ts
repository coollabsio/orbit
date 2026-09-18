import { expect, test } from 'bun:test'

test('the collapse toggle fades in on sidebar hover and is always reachable by keyboard', async () => {
  const css = await Bun.file(new URL('./shell.css', import.meta.url)).text()
  const toggle = css.slice(css.indexOf('.app-sidebar-collapse {'), css.indexOf('.app-sidebar-title {'))

  expect(toggle).toContain('opacity: 0;')
  expect(toggle).toContain('pointer-events: none;')
  expect(toggle).toContain('transition: opacity 0.14s ease, color 0.14s ease;')
  expect(toggle).toMatch(/\.app-sidebar:hover \.app-sidebar-collapse,\s*\n\.app-sidebar-collapse:focus-visible \{[^}]*opacity: 1;/)
})

test('the collapsed rail swaps the workspace initial for the expand icon in place', async () => {
  const css = await Bun.file(new URL('./shell.css', import.meta.url)).text()

  expect(css).toMatch(/\.app-sidebar\[data-collapsed='true'\] \.app-sidebar-collapse \{[^}]*position: absolute;/)
  expect(css).toMatch(/\.app-sidebar\[data-collapsed='true'\]:hover \.workspace-switcher[\s\S]{0,160}opacity: 0;/)
  expect(css).toContain(":has(.app-sidebar-collapse:focus-visible)")
  expect(css).toMatch(/\.app-sidebar\[data-collapsed='true'\] \.app-sidebar-collapse svg \{\s*transform: rotate\(180deg\);/)
})

test('reduced motion drops the fade instead of animating it', async () => {
  const css = await Bun.file(new URL('./shell.css', import.meta.url)).text()
  const reduced = css.slice(css.indexOf('@media (prefers-reduced-motion: reduce)'))

  expect(reduced).toContain('.app-sidebar-collapse,')
  expect(reduced).toContain('.workspace-switcher {')
  expect(reduced).toContain('transition: none;')
})
