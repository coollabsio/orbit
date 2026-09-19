import { expect, test } from 'bun:test'

test('the topbar reads scope then view: project picker, then the active view title', async () => {
  const source = await Bun.file(new URL('./TasksPage.tsx', import.meta.url)).text()

  expect(source).not.toContain('<ProjectRail')
  expect(source).not.toContain('pane-header product-pane-header')

  const left = source.slice(source.indexOf('<TopbarSlot side="left">'), source.indexOf('<TopbarSlot side="right">'))
  expect(left).toContain('tasks-project-picker')
  expect(left).toContain("{activeProject?.name ?? 'All projects'}")
  // The view (My tasks, Overdue, This week, ...) is the current crumb, after the picker.
  expect(left.indexOf('tasks-project-picker')).toBeLessThan(left.indexOf('>{viewTitle}</span>'))
  expect(left.indexOf('>New project</button>')).toBeLessThan(left.indexOf('{projectSettingsLabel()}</button>'))
})

test('new projects are created in a modal rather than a browser prompt', async () => {
  const source = await Bun.file(new URL('./TasksPage.tsx', import.meta.url)).text()

  expect(source).toContain('setShowNewProject(true)')
  expect(source).toContain('<NewProjectModal')
  expect(source).not.toContain('window.prompt')
})

test('filters and the create action live in the right-hand topbar slot', async () => {
  const source = await Bun.file(new URL('./TasksPage.tsx', import.meta.url)).text()
  const right = source.slice(source.indexOf('<TopbarSlot side="right">'), source.indexOf('<div className="pane-body">'))

  expect(right).toContain('<TaskFilters')
  expect(right).toContain('aria-label="New task"')
  // Creation now opens the modal; the create failure message lives there, not the topbar.
  expect(right).not.toContain('Task creation failed.')
})

test('the New task button opens the create modal instead of creating then navigating', async () => {
  const source = await Bun.file(new URL('./TasksPage.tsx', import.meta.url)).text()

  expect(source).toContain('<NewTaskModal')
  expect(source).toContain('onClick={() => openNewTask()}')
  // No immediate "Untitled" create-and-navigate anymore.
  expect(source).not.toContain("title: 'Untitled'")
})

test('the page-local hamburger is gone now the topbar is global', async () => {
  const [source, css] = await Promise.all([
    Bun.file(new URL('./TasksPage.tsx', import.meta.url)).text(),
    Bun.file(new URL('./tasks.css', import.meta.url)).text(),
  ])

  expect(source).not.toContain('tasks-sidebar-button')
  expect(source).not.toContain("new CustomEvent('open-sidebar')")
  expect(css).not.toContain('.tasks-sidebar-button')
})
