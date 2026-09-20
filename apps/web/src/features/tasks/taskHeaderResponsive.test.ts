import { expect, test } from 'bun:test'

test('mobile task header wraps search without pushing action buttons off screen', async () => {
  const [page, filters] = await Promise.all([
    Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text(),
    Bun.file(new URL('./components/TaskFilters.tsx', import.meta.url)).text(),
  ])
  // the header itself wraps on mobile
  expect(page).toContain('max-[899px]:flex-wrap')
  // the search field drops to its own full-width row after the action buttons
  expect(filters).toContain('max-[899px]:order-10')
  expect(filters).toContain('max-[899px]:basis-full')
})
