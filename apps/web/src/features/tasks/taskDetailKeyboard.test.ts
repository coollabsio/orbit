import { expect, test } from 'bun:test'

test('Escape closes task detail on its redirect or task project', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()
  const effect = source.slice(source.indexOf("if (!taskId) return"), source.indexOf('const creating = useRef'))

  expect(effect).toContain("event.key === 'Escape'")
  expect(effect).toContain('navigate(redirect ?? `/tasks${closeSearchSuffix}`)')
  expect(effect).toContain("document.addEventListener('keydown', onKeyDown)")
  expect(effect).toContain("document.removeEventListener('keydown', onKeyDown)")
})

test('task detail URLs omit the project query parameter', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()

  expect(source).toContain("detailParams.delete('project')")
  expect(source).toContain("if (taskProjectId) closeParams.set('project', taskProjectId)")
  expect(source).toContain('navigate(`/tasks/${id}${detailSearchSuffix}`)')
  expect(source).toContain('navigate(`/tasks/${taskId}${detailSearchSuffix}`, { replace: true })')
})

test('task detail opens related tasks through the page navigation', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()
  expect(source).toContain('onOpenTask={openTask}')
})

test('the open task resolves relation identifiers against every project', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()
  expect(source).toContain('activityQuery.data, projects)')
})
