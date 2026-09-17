import { expect, test } from 'bun:test'

test('keeps the task identifier on one line', async () => {
  const css = await Bun.file(new URL('./tasks.css', import.meta.url)).text()
  const rule = css.match(/\.tasks-row-id\s*\{([^}]*)\}/)?.[1]

  expect(rule).toContain('white-space: nowrap')
})
