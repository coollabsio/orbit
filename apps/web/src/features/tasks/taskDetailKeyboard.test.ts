import { expect, test } from 'bun:test'

test('Escape closes task detail on its task project', async () => {
  const source = await Bun.file(new URL('./TasksPage.tsx', import.meta.url)).text()
  const effect = source.slice(source.indexOf("if (!taskId) return"), source.indexOf('const creating = useRef'))

  // Which Escapes count (not one a menu or editor already handled) is covered by
  // closeOnEscape.test.tsx; here we pin that the page actually uses that guard.
  expect(effect).toContain('shouldCloseTaskOnKey(event)')
  expect(effect).toContain('navigate(`/tasks${closeSearchSuffix}`)')
  expect(effect).toContain("document.addEventListener('keydown', onKeyDown)")
  expect(effect).toContain("document.removeEventListener('keydown', onKeyDown)")
})

test('task detail URLs omit the project query parameter', async () => {
  const source = await Bun.file(new URL('./TasksPage.tsx', import.meta.url)).text()

  expect(source).toContain("detailParams.delete('project')")
  expect(source).toContain("if (taskProjectId) closeParams.set('project', taskProjectId)")
  expect(source).toContain('navigate(`/tasks/${id}${detailSearchSuffix}`)')
  expect(source).toContain('navigate(`/tasks/${taskId}${detailSearchSuffix}`, { replace: true })')
})
