import { expect, test } from 'bun:test'
import { projectDraft } from './projectDraft'

test('project creation derives a stable key and default token color', () => {
  expect(projectDraft('Mobile App')).toEqual({ name: 'Mobile App', key: 'MOBIL', color: '#8b5cf6' })
  expect(projectDraft('---')).toEqual({ name: '---', key: 'PROJ', color: '#8b5cf6' })
})
