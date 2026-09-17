import { expect, test } from 'bun:test'

test('Escape closes task detail and preserves list filters', async () => {
  const source = await Bun.file(new URL('./TasksPage.tsx', import.meta.url)).text()
  const effect = source.slice(source.indexOf("if (!taskId) return"), source.indexOf('const creating = useRef'))

  expect(effect).toContain("event.key === 'Escape'")
  expect(effect).toContain('navigate(`/tasks${searchSuffix}`)')
  expect(effect).toContain("document.addEventListener('keydown', onKeyDown)")
  expect(effect).toContain("document.removeEventListener('keydown', onKeyDown)")
})
