import { expect, test } from 'bun:test'

test('changing a task view clears the task search', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()

  expect(source).toContain("searchFilter: ''")
  expect(source).toContain('lastView.current !== viewFilter')
  expect(source).toContain('}, [viewFilter, setPreferences])')
})
