import { expect, test } from 'bun:test'

test('mobile task header wraps search without pushing action buttons off screen', async () => {
  const css = await Bun.file(new URL('./tasks.css', import.meta.url)).text()
  const mobile = css.slice(css.indexOf('@media (max-width: 899px)'), css.indexOf('@media (max-width: 480px)'))
  expect(mobile).toContain('flex-wrap: wrap')
  expect(mobile).toContain('.tasks-search {\n    order: 10;')
  expect(mobile).toContain('flex-basis: 100%')
})
