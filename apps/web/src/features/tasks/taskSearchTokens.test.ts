import { expect, test } from 'bun:test'

test('task search uses shared input tokens and one wrapper focus ring', async () => {
  const source = await Bun.file(new URL('./components/TaskFilters.tsx', import.meta.url)).text()
  const wrapper = source.slice(source.indexOf('const SEARCH_GROUP'), source.indexOf('interface TaskFiltersProps'))
  // the InputGroup wrapper carries the shared control tokens and the single focus ring
  expect(wrapper).toContain('border border-input')
  expect(wrapper).toContain('bg-muted')
  // the search sits beside the filter buttons: override InputGroup's own `w-full`, or it eats the header row
  expect(wrapper).toContain('w-auto')
  expect(wrapper).toContain('has-[[data-slot=input-group-control]:focus-visible]:ring-1')
  // the inner input contributes no border/ring of its own
  const group = source.slice(source.indexOf('<InputGroup'), source.indexOf('</InputGroup>'))
  expect(group).toContain('border-0')
  expect(group).toContain('focus-visible:ring-0')
})
