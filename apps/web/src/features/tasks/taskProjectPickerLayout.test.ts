import { expect, test } from 'bun:test'

test('shows the project picker before the active task view heading without a project rail', async () => {
  const source = await Bun.file(new URL('./TasksPage.tsx', import.meta.url)).text()

  expect(source).not.toContain('<ProjectRail')

  const header = source.slice(source.indexOf('<div className="pane-header product-pane-header">'), source.indexOf('<div className="spacer" />'))
  expect(header.indexOf('tasks-project-picker')).toBeLessThan(header.indexOf('>{viewTitle}</span>'))
  expect(header.indexOf('>New project</button>')).toBeLessThan(header.indexOf('{projectSettingsLabel()}</button>'))
})
