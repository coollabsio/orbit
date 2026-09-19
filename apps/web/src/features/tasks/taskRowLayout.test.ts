import { expect, test } from 'bun:test'

test('keeps the task identifier on one line', async () => {
  const source = await Bun.file(new URL('./components/TaskRow.tsx', import.meta.url)).text()
  // the identifier cell renders with a nowrap class so the id never wraps
  const idCell = source.slice(0, source.indexOf('{task.identifier}</span>'))
  const openingTag = idCell.slice(idCell.lastIndexOf('<span'))

  expect(openingTag).toContain('whitespace-nowrap')
})
