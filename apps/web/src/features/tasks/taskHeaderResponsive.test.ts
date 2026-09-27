import { expect, test } from 'bun:test'

test('mobile task header wraps search without pushing action buttons off screen', async () => {
  const [page, search] = await Promise.all([
    Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text(),
    Bun.file(new URL('./components/TaskSearchBox.tsx', import.meta.url)).text(),
  ])
  // the header itself wraps on mobile
  expect(page).toContain('max-[899px]:flex-wrap')
  // the search field drops to its own full-width row after the action buttons
  expect(search).toContain('max-[899px]:order-10')
  expect(search).toContain('max-[899px]:basis-full')
})

test('task detail shows back arrow on mobile and close X on desktop', async () => {
  const detail = await Bun.file(new URL('./components/TaskDetail.tsx', import.meta.url)).text()
  expect(detail).toContain('min-[900px]:hidden" onClick={onBack} aria-label="Back to tasks"')
  expect(detail).toContain('max-[899px]:hidden" onClick={onBack} aria-label="Close task"')
})
