import { expect, test } from 'bun:test'

test('task search uses shared input tokens and one wrapper focus ring', async () => {
  const css = await Bun.file(new URL('./tasks.css', import.meta.url)).text()
  expect(css).toContain('border: 1px solid var(--control-border)')
  expect(css).toContain('background: var(--recessed)')
  expect(css).toContain('color: var(--text-primary)')
  expect(css).toContain('.tasks-search:focus-within')
  expect(css).toContain('.tasks-search input:focus-visible')
  expect(css).toContain('box-shadow: none')
})
