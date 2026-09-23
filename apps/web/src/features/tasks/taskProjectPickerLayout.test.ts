import { expect, test } from 'bun:test'

test('shows a visible settings gear on each project without a separate settings item', async () => {
  const source = await Bun.file(new URL('./pages/TasksPage.tsx', import.meta.url)).text()

  expect(source).not.toContain('<ProjectRail')

  // the project picker (Select project) renders before the view title heading
  expect(source.indexOf('aria-label="Select project"')).toBeLessThan(source.indexOf('{viewTitle}</span>'))
  expect(source).toContain('>New project</DropdownMenuItem>')
  expect(source).not.toContain('{projectSettingsLabel()}</DropdownMenuItem>')
  expect(source).toContain('aria-label={`${project.name} settings`}')
  expect(source).toContain('onClick={() => navigate(`/tasks/projects/${project.id}/settings`)}')
  const gearClasses = source.match(/<DropdownMenuItem className="([^"]+)" aria-label=\{`\$\{project.name\} settings`\}/)?.[1]
  expect(gearClasses).toBeDefined()
  expect(gearClasses).not.toContain('opacity-0')
})

test('keeps the project rail settings gear visible without hover', async () => {
  const source = await Bun.file(new URL('./components/ProjectRail.tsx', import.meta.url)).text()

  const gearClasses = source.match(/className="([^"]+)"\s+aria-label=\{`\$\{project.name\} settings`\}/)?.[1]
  expect(gearClasses).toBeDefined()
  expect(gearClasses).not.toContain('opacity-0')
})
