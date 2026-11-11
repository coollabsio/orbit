import { describe, expect, test } from 'bun:test'
import { editedDate, localParts } from './dates'

describe('editedDate', () => {
  test('a date range keeps its end', () => {
    expect(editedDate({ start: '2026-11-09', end: '2026-11-12', withTime: false, startTime: '09:00', endTime: '09:00' })).toEqual({
      start: { kind: 'day', day: '2026-11-09' },
      end: { kind: 'day', day: '2026-11-12' },
    })
  })

  test('an end before the start is dropped', () => {
    expect(editedDate({ start: '2026-11-09', end: '2026-11-01', withTime: false, startTime: '09:00', endTime: '09:00' }).end).toBe(null)
    expect(editedDate({ start: '2026-11-09', end: '2026-11-09', withTime: true, startTime: '10:00', endTime: '09:30' }).end).toBe(null)
  })

  test('a time round-trips through the local parts', () => {
    const date = editedDate({ start: '2026-11-09', end: null, withTime: true, startTime: '14:05', endTime: '14:05' })
    expect(date.start.kind).toBe('moment')
    expect(localParts(date.start)).toEqual({ day: '2026-11-09', time: '14:05' })
  })
})
