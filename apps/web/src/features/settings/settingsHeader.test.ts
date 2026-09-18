import { expect, test } from 'bun:test'

test('settings keeps its own title on mobile now the topbar is route-agnostic', async () => {
  const css = await Bun.file(new URL('./settings.css', import.meta.url)).text()

  expect(css).not.toMatch(/@media \(max-width: 899px\)\s*\{\s*\.settings-page-header\s*\{\s*display: none;/)
  expect(css).toContain('.settings-page-header .product-pane-header-icon')
})
