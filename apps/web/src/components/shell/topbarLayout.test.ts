import { expect, test } from 'bun:test'

test('the topbar is a real 48px bar on every route at every width', async () => {
  const css = await Bun.file(new URL('./shell.css', import.meta.url)).text()
  const topbar = css.slice(css.indexOf('.topbar {'), css.indexOf('.topbar-menu-button {'))

  expect(topbar).toContain('display: flex;')
  expect(topbar).not.toContain('display: none;')
  expect(topbar).toContain('height: var(--topbar-height);')
  expect(topbar).toContain('border-bottom: 1px solid var(--hairline);')
})

test('slot containers line the breadcrumb up left and the page controls right', async () => {
  const css = await Bun.file(new URL('./shell.css', import.meta.url)).text()

  expect(css).toMatch(/\.topbar-slot\[data-slot='left'\] \{[^}]*display: contents;/)
  expect(css).toMatch(/\.topbar-slot\[data-slot='right'\] \{[^}]*display: flex;/)
  expect(css).toMatch(/\.topbar-crumbs \{[^}]*flex: 1;/)
})

test('no per-route topbar variants survive and mobile keeps one compact wrapping bar', async () => {
  const css = await Bun.file(new URL('./shell.css', import.meta.url)).text()
  const mobile = css.slice(css.indexOf('@media (max-width: 899px)'))

  expect(css).not.toContain("data-root=")
  expect(mobile).toMatch(/\.topbar \{[^}]*min-height: 44px;/)
  expect(mobile).toMatch(/\.topbar \{[^}]*flex-wrap: wrap;/)
  expect(mobile).toMatch(/\.topbar \{[^}]*border-bottom: none;/)
  expect(mobile).toMatch(/\.topbar-slot\[data-slot='right'\] \{[^}]*flex-wrap: wrap;/)
})
