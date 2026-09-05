import { expect, test } from 'bun:test'
import { projectDraft, projectSettingsLabel, projectSettingsPath } from './projectDraft'

test('project creation derives a stable key and default token color', () => {
  expect(projectDraft('Mobile App')).toEqual({ name: 'Mobile App', key: 'MOBIL', color: '#8b5cf6' })
  expect(projectDraft('---')).toEqual({ name: '---', key: 'PROJ', color: '#8b5cf6' })
})

test('selected projects have a settings route for rename and delete', () => {
  expect(projectSettingsPath('project-1')).toBe('/tasks/projects/project-1/settings')
  expect(projectSettingsPath(null)).toBeNull()
  expect(projectSettingsLabel()).toBe('Project settings')
})
