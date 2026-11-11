import { expect, test } from 'bun:test'
import { namesDestination, paletteArea, sectionOrder } from './paletteOrder'

test('paletteArea maps a path to its part of the app', () => {
  expect(paletteArea('/tasks/ENG-1')).toBe('tasks')
  expect(paletteArea('/views/abc')).toBe('tasks')
  expect(paletteArea('/docs/page-1')).toBe('docs')
  expect(paletteArea('/chat/c1')).toBe('chat')
  expect(paletteArea('/settings')).toBe('other')
  expect(paletteArea('/tasksy')).toBe('other')
})

test('sectionOrder puts the part in view first, a task ID puts tasks first, a destination name puts navigation first', () => {
  expect(sectionOrder('docs', 'roadmap', false)).toEqual(['docs', 'tasks', 'chat', 'navigation'])
  expect(sectionOrder('chat', 'roadmap', false)).toEqual(['chat', 'tasks', 'docs', 'navigation'])
  expect(sectionOrder('docs', 'ENG-12', false)).toEqual(['tasks', 'docs', 'chat', 'navigation'])
  expect(sectionOrder('docs', '#12', true)).toEqual(['tasks', 'docs', 'chat', 'navigation'])
  expect(sectionOrder('tasks', 'set', true)).toEqual(['navigation', 'tasks', 'docs', 'chat'])
})

test('namesDestination matches the start of a word of the name', () => {
  expect(namesDestination('Task trash', 'tra')).toBe(true)
  expect(namesDestination('Settings', 'SET')).toBe(true)
  expect(namesDestination('Settings', 'ttings')).toBe(false)
  expect(namesDestination('Settings', ' ')).toBe(false)
})
