import { describe, expect, test } from 'bun:test'
import { defaultTemplate, templateFill, templatesOf } from './templates'
import { ctx, database } from './testing'

const template = (id: string, position: number, values: Record<string, unknown>) => ({
  id,
  name: `Template ${id}`,
  title: 'From a template',
  icon: null,
  position,
  version: 0,
  has_content: true,
  created_at: '2026-11-10T00:00:00Z',
  created_by: 'u-me',
  updated_at: '2026-11-10T00:00:00Z',
  updated_by: 'u-me',
  values,
})

describe('row templates', () => {
  const templates = templatesOf({
    properties: database.properties,
    templates: [
      template('t-b', 1, { 'p-text': 'Second' }),
      template('t-a', 0, { 'p-person': ['me', 'u-bob'], 'p-date': { start: 'today', end: null }, 'p-num': 3 }),
    ],
  })

  test('parse in order, keep "me" among the people and list "today" dates apart', () => {
    expect(templates.map((parsed) => parsed.id)).toEqual(['t-a', 't-b'])
    expect(templates[0]).toMatchObject({
      name: 'Template t-a',
      today: ['p-date'],
      values: { 'p-person': { kind: 'people', ids: ['me', 'u-bob'] }, 'p-num': { kind: 'number', number: 3 } },
    })
    expect(templates[0]?.values['p-date']).toBeUndefined()
    expect(templates[1]?.today).toEqual([])
  })

  test('the default template of a view, only while it exists', () => {
    expect(defaultTemplate({ default_template_id: 't-b' }, templates)?.name).toBe('Template t-b')
    expect(defaultTemplate({ default_template_id: 't-gone' }, templates)).toBeNull()
    expect(defaultTemplate({}, templates)).toBeNull()
  })

  test('a row from a template sends the viewer\'s local day', () => {
    // 2026-11-10T23:30Z is already the 11th in Budapest.
    const late = { now: Date.UTC(2026, 10, 10, 23, 30), timeZone: 'Europe/Budapest' }
    expect(templateFill('t-a', late)).toEqual({ template_id: 't-a', today: '2026-11-11' })
    expect(templateFill('t-a', { ...late, timeZone: 'UTC' })).toEqual({ template_id: 't-a', today: '2026-11-10' })
    expect(templateFill(null, ctx)).toEqual({})
  })
})
