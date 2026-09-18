import { expect, test } from 'bun:test'

test('the project picker is the terminal breadcrumb segment in the topbar', async () => {
  const source = await Bun.file(new URL('./TasksPage.tsx', import.meta.url)).text()

  expect(source).not.toContain('<ProjectRail')
  expect(source).not.toContain('pane-header product-pane-header')

  const left = source.slice(source.indexOf('<TopbarSlot side="left">'), source.indexOf('<TopbarSlot side="right">'))
  expect(left).toContain('tasks-project-picker')
  expect(left).toContain("{activeProject?.name ?? 'All tasks'}")
  expect(left.indexOf('>New project</button>')).toBeLessThan(left.indexOf('{projectSettingsLabel()}</button>'))
})

test('filters and the create action live in the right-hand topbar slot', async () => {
  const source = await Bun.file(new URL('./TasksPage.tsx', import.meta.url)).text()
  const right = source.slice(source.indexOf('<TopbarSlot side="right">'), source.indexOf('<div className="pane-body">'))

  expect(right).toContain('<TaskFilters')
  expect(right).toContain('aria-label="New task"')
  expect(right).toContain('Task creation failed.')
  expect(right).toContain('Project creation failed.')
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
