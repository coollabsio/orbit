import { describe, expect, test } from 'bun:test'
import { fieldActivityText, fieldValueText, newFieldKinds, parseFieldInput, selectedOptions, timeTrackingField } from './taskFields'

const options = [{ id: 'a', name: 'Acme', color: '#ff0000' }, { id: 'b', name: 'Globex', color: '#00ff00' }]

describe('custom field values', () => {
  test('parses text inputs by kind', () => {
    expect(parseFieldInput('number', ' 1,250.5 ')).toBe(1250.5)
    expect(parseFieldInput('number', 'twelve')).toBeUndefined()
    expect(parseFieldInput('number', '1e16')).toBeUndefined()
    expect(parseFieldInput('text', '   ')).toBeNull()
    expect(parseFieldInput('url', 'https://example.com/x')).toBe('https://example.com/x')
    expect(parseFieldInput('url', 'javascript:alert(1)')).toBeUndefined()
    expect(parseFieldInput('url', 'not a url')).toBeUndefined()
  })

  test('shows only the options that still exist, in the field order', () => {
    const field = { kind: 'multi_select' as const, options }
    expect(selectedOptions(field, ['b', 'gone', 'a']).map((option) => option.id)).toEqual(['a', 'b'])
    expect(fieldValueText(field, ['b', 'gone'])).toBe('Globex')
    expect(fieldValueText({ kind: 'select', options }, 'gone')).toBe('')
    expect(fieldValueText({ kind: 'checkbox', options: [] }, true)).toBe('Yes')
    expect(fieldValueText({ kind: 'date', options: [] }, '2026-03-04')).toContain('2026')
  })

  test('shows logged minutes as a duration', () => {
    expect(fieldValueText({ kind: 'time_tracking', options: [] }, 90)).toBe('1h 30m')
    expect(fieldValueText({ kind: 'time_tracking', options: [] }, 0)).toBe('')
  })

  test('offers time tracking once per workspace and finds it in a project', () => {
    const text = { id: 't', kind: 'text' as const }
    const time = { id: 'h', kind: 'time_tracking' as const }
    expect(newFieldKinds([text]).map((item) => item.kind)).toContain('time_tracking')
    expect(newFieldKinds([text, time]).map((item) => item.kind)).not.toContain('time_tracking')
    expect(timeTrackingField([text, time])).toBe(time)
    expect(timeTrackingField([text])).toBeUndefined()
  })

  test('names the changed field in the activity', () => {
    expect(fieldActivityText('task.field_changed', { field_name: 'Budget' })).toBe('changed Budget')
    expect(fieldActivityText('task.updated', { field_name: 'Budget' })).toBeNull()
  })
})
