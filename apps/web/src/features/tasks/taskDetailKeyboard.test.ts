import { expect, test } from 'bun:test'

test('Escape closes task detail on its redirect or the page it was opened from', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()
  const effect = source.slice(source.indexOf("if (!taskId) return"), source.indexOf('const creating = useRef'))

  expect(effect).toContain("event.key === 'Escape'")
  expect(effect).toContain('navigate(redirect ?? `${basePath}${closeSearchSuffix}`)')
  expect(effect).toContain("document.addEventListener('keydown', onKeyDown)")
  expect(effect).toContain("document.removeEventListener('keydown', onKeyDown)")
})

test('task detail URLs omit the project query parameter', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()

  expect(source).toContain("detailParams.delete('project')")
  // closing returns to the page the task was opened from, never to the task's project page
  // (behaviour covered in pages/TasksPage.test.tsx)
  expect(source).not.toContain("closeParams.set('project', taskProjectId)")
  expect(source).toContain('navigate(`${basePath}/${id}${detailSearchSuffix}`')
  expect(source).toContain('navigate(`${basePath}/${taskId}${detailSearchSuffix}`, { replace: true')
})

test('saved views open tasks under their own path', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()
  expect(source).toContain("const basePath = viewId ? `/views/${viewId}` : '/tasks'")
})

test('task detail opens related tasks through the page navigation', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()
  expect(source).toContain('onOpenTask={openTask}')
})

test('the open task resolves relation identifiers against every project', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()
  expect(source).toContain('activityQuery.data, projects)')
})
