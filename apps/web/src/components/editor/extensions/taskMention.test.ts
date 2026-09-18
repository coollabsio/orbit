import { expect, test } from 'bun:test'
import { TaskMention } from './taskMention'

test('the node is an inline atom named taskMention so it never splits mid-chip', () => {
  expect(TaskMention.name).toBe('taskMention')
  expect(TaskMention.config.group).toBe('inline')
  expect(TaskMention.config.inline).toBe(true)
  expect(TaskMention.config.atom).toBe(true)
  expect(TaskMention.config.selectable).toBe(true)
})

test('it declares exactly the two allowlisted attributes', () => {
  const attributes = TaskMention.config.addAttributes?.call({ name: 'taskMention' } as never) ?? {}

  expect(Object.keys(attributes).sort()).toEqual(['id', 'identifier'])
})

test('attributes round trip through the data-* html representation', () => {
  const attributes = TaskMention.config.addAttributes?.call({ name: 'taskMention' } as never) as Record<
    string,
    { default: unknown; parseHTML: (element: HTMLElement) => unknown; renderHTML: (attrs: Record<string, unknown>) => Record<string, unknown> }
  >
  const element = document.createElement('span')
  element.setAttribute('data-id', 'task-1')
  element.setAttribute('data-identifier', 'ORB-12')

  expect(attributes.id.parseHTML(element)).toBe('task-1')
  expect(attributes.identifier.parseHTML(element)).toBe('ORB-12')
  expect(attributes.id.renderHTML({ id: 'task-1' })).toEqual({ 'data-id': 'task-1' })
  expect(attributes.identifier.renderHTML({ identifier: 'ORB-12' })).toEqual({ 'data-identifier': 'ORB-12' })
  expect(attributes.id.renderHTML({ id: null })).toEqual({})
})

test('renderHTML emits the marker attribute the parse rule looks for', () => {
  const rendered = TaskMention.config.renderHTML?.call({ name: 'taskMention', options: {} } as never, {
    node: { attrs: { id: 'task-1', identifier: 'ORB-12' } },
    HTMLAttributes: { 'data-id': 'task-1', 'data-identifier': 'ORB-12' },
  } as never) as [string, Record<string, unknown>, string]

  expect(rendered[0]).toBe('span')
  expect(rendered[1]['data-task-mention']).toBe('')
  expect(rendered[2]).toBe('ORB-12')
})

test('the parse rule only claims spans carrying the marker attribute', () => {
  const rules = TaskMention.config.parseHTML?.call({ name: 'taskMention' } as never) ?? []

  expect(rules).toEqual([{ tag: 'span[data-task-mention]' }])
})
