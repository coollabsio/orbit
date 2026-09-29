import { expect, test } from 'bun:test'

test('task search keeps the InputGroup look and only overrides its width', async () => {
  const source = await Bun.file(new URL('./components/TaskSearchBox.tsx', import.meta.url)).text()
  const group = source.slice(source.indexOf('<InputGroup'), source.indexOf('</InputGroup>'))
  // the search sits beside the filter buttons: override InputGroup's own `w-full`, or it eats the header row
  expect(group).toContain('w-auto')
  // the shared control look (border, focus ring) comes from InputGroup itself
  expect(group).not.toContain('border-input')
  expect(group).not.toContain('focus-visible]:ring')
})
