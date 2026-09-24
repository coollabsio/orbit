import { expect, test } from 'bun:test'

test('saved views render the task page, with task detail under the view', async () => {
  const source = await Bun.file(new URL('./App.tsx', import.meta.url)).text()
  expect(source).toContain('<Route path="views/:viewId" element={<TasksPage />} />')
  expect(source).toContain('<Route path="views/:viewId/:taskId" element={<TasksPage />} />')
})

test('below 900px the shell topbar hides on view pages, which bring their own header', async () => {
  const source = await Bun.file(new URL('./shell/Topbar.tsx', import.meta.url)).text()
  expect(source).toContain('data-[root=views]:hidden')
  expect(source).toContain("case 'views':")
})
