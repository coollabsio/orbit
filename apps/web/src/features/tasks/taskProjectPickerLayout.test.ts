import { expect, test } from 'bun:test'

test('shows the project picker before the active task view heading without a project rail', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()

  expect(source).not.toContain('<ProjectRail')

  // the project picker (Select project) renders before the view title heading
  expect(source.indexOf('aria-label="Select project"')).toBeLessThan(source.indexOf('{viewTitle}</span>'))
  // inside the picker menu, "New project" comes before the project-settings entry
  expect(source.indexOf('>New project</DropdownMenuItem>')).toBeLessThan(source.indexOf('{projectSettingsLabel()}</DropdownMenuItem>'))
})
