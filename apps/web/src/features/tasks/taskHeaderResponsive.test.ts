import { expect, test } from 'bun:test'

test('the compact topbar wraps the task search onto its own row', async () => {
  const [shell, tasks] = await Promise.all([
    Bun.file(new URL('../../components/shell/shell.css', import.meta.url)).text(),
    Bun.file(new URL('./tasks.css', import.meta.url)).text(),
  ])
  const shellMobile = shell.slice(shell.indexOf('@media (max-width: 899px)'))
  const tasksMobile = tasks.slice(tasks.indexOf('@media (max-width: 899px)'), tasks.indexOf('@media (max-width: 480px)'))

  expect(shellMobile).toMatch(/\.topbar \{[^}]*flex-wrap: wrap;/)
  expect(shellMobile).toMatch(/\.topbar-slot\[data-slot='right'\] \{[^}]*flex-wrap: wrap;/)
  expect(tasksMobile).toContain('.tasks-search {\n    order: 10;')
  expect(tasksMobile).toContain('flex-basis: 100%')
})

test('task header rules no longer reference the removed pane header', async () => {
  const tasks = await Bun.file(new URL('./tasks.css', import.meta.url)).text()

  expect(tasks).not.toContain('.tasks-list-pane > .pane-header')
})

test('compact filter and create buttons stay icon-only on mobile', async () => {
  const tasks = await Bun.file(new URL('./tasks.css', import.meta.url)).text()
  const tasksMobile = tasks.slice(tasks.indexOf('@media (max-width: 899px)'), tasks.indexOf('@media (max-width: 480px)'))

  expect(tasksMobile).toMatch(/\.tasks-filter,\s*\n\s*\.topbar-slot\[data-slot='right'\] > \.button-primary \{/)
  expect(tasksMobile).toContain('.tasks-filter-label,\n  .tasks-new-label {')
})
