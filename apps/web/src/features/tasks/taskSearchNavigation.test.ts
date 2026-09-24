import { expect, test } from 'bun:test'

test('changing a preset or saved view clears the local quick search', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()

  expect(source).toContain("setSearch('')")
  expect(source).toContain('lastScope.current !== scopeKey')
  expect(source).toContain('}, [scopeKey])')
})

test('quick search filters the loaded tasks locally and is never saved', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()

  expect(source).toContain('quickSearchTasks(tasks, search)')
  expect(source).not.toContain('useTaskPreferences')
  expect(source).not.toContain("searchParams.get('layout')")
  expect(source).not.toContain('orbit:task_layout')
})
