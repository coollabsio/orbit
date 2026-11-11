import { describe, expect, test } from 'bun:test'
import { normalizeRecurrence, recurrenceLabel } from './recurrence'

describe('recurrenceLabel', () => {
  test('names the rule', () => {
    expect(recurrenceLabel(null)).toBe('Does not repeat')
    expect(recurrenceLabel({ frequency: 'daily', interval: 1, weekdays: [], base: 'due' })).toBe('Every day')
    expect(recurrenceLabel({ frequency: 'weekly', interval: 1, weekdays: ['wed', 'mon'], base: 'due' })).toBe('Every week on Mon, Wed')
    expect(recurrenceLabel({ frequency: 'weekly', interval: 1, weekdays: ['mon', 'tue', 'wed', 'thu', 'fri'], base: 'due' })).toBe('Every week on weekdays')
    expect(recurrenceLabel({ frequency: 'monthly', interval: 2, weekdays: [], base: 'completion' })).toBe('Every 2 months · from completion')
  })
})

describe('normalizeRecurrence', () => {
  test('drops weekdays the server refuses and clamps the interval', () => {
    expect(normalizeRecurrence({ frequency: 'weekly', interval: 0, weekdays: ['fri', 'mon'], base: 'completion' })).toEqual({ frequency: 'weekly', interval: 1, weekdays: [], base: 'completion' })
    expect(normalizeRecurrence({ frequency: 'weekly', interval: 900, weekdays: ['fri', 'mon'], base: 'due' })).toEqual({ frequency: 'weekly', interval: 365, weekdays: ['mon', 'fri'], base: 'due' })
  })
})
