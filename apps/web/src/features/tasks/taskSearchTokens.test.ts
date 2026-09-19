import { expect, test } from 'bun:test'

test('task search uses shared input tokens and one wrapper focus ring', async () => {
  const source = await Bun.file(new URL('./components/TaskFilters.tsx', import.meta.url)).text()
  const wrapper = source.slice(source.indexOf('<label'), source.indexOf('</label>'))
  // the wrapper carries the shared control tokens and the single focus ring
  expect(wrapper).toContain('border border-input')
  expect(wrapper).toContain('bg-muted')
  expect(wrapper).toContain('focus-within:ring-1')
  // the inner input contributes no border/ring of its own
  expect(wrapper).toContain('border-0')
  expect(wrapper).toContain('focus:outline-none')
})
