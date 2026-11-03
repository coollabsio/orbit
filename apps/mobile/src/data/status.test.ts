import { expect, test } from 'bun:test'

import { canSaveCustomStatus, clearAfterOptions, clearAfterTime, currentStatus, customStatusBody, withoutCustomStatus, withPresence } from './status'

const now = Date.parse('2026-10-25T12:00:00.000Z')

test('a custom status shows until its end; after it, only the presence is left', () => {
  const status = { presence: 'dnd' as const, emoji: '🌴', text: 'Away', expires_at: '2026-10-25T13:00:00.000Z' }
  expect(currentStatus(status, now)).toEqual({ presence: 'dnd', emoji: '🌴', text: 'Away', expiresAt: '2026-10-25T13:00:00.000Z' })
  expect(currentStatus(status, Date.parse('2026-10-25T13:00:00.000Z'))).toEqual({ presence: 'dnd', emoji: null, text: null, expiresAt: null })
})

test('no end never passes, an unreadable end has passed, empty parts are none', () => {
  expect(currentStatus({ presence: 'idle', text: 'Lunch' }, now)).toEqual({ presence: 'idle', emoji: null, text: 'Lunch', expiresAt: null })
  expect(currentStatus({ presence: 'online', text: 'Lunch', expires_at: 'soon' }, now)).toEqual({ presence: 'online', emoji: null, text: null, expiresAt: null })
  expect(currentStatus({ presence: 'online', emoji: '', text: '', expires_at: '2026-10-26T00:00:00.000Z' }, now)).toEqual({ presence: 'online', emoji: null, text: null, expiresAt: null })
  expect(currentStatus(undefined, now).presence).toBe('online')
})

test('a presence change keeps the custom status; clearing keeps the presence', () => {
  const status = currentStatus({ presence: 'online', emoji: '🎧', text: 'Focus', expires_at: '2026-10-25T13:00:00.000Z' }, now)
  expect(withPresence(status, 'invisible')).toEqual({ presence: 'invisible', emoji: '🎧', text: 'Focus', expires_at: '2026-10-25T13:00:00.000Z' })
  expect(withoutCustomStatus(status)).toEqual({ presence: 'online', emoji: null, text: null, expires_at: null })
})

test('Clear after offers to keep an end that is set, first', () => {
  expect(clearAfterOptions(null, () => 'x').map((option) => option.value)).toEqual(['never', '30m', '1h', '4h', 'today'])
  const options = clearAfterOptions('2026-10-25T13:00:00.000Z', () => 'Oct 25, 1:00 PM')
  expect(options[0]).toEqual({ value: 'keep', label: 'Keep (Oct 25, 1:00 PM)' })
  expect(options).toHaveLength(6)
})

test('the end of each Clear after choice', () => {
  const at = new Date(now)
  expect(clearAfterTime('keep', '2026-10-25T13:00:00.000Z', at)).toBe('2026-10-25T13:00:00.000Z')
  expect(clearAfterTime('never', '2026-10-25T13:00:00.000Z', at)).toBeNull()
  expect(clearAfterTime('30m', null, at)).toBe('2026-10-25T12:30:00.000Z')
  expect(clearAfterTime('1h', null, at)).toBe('2026-10-25T13:00:00.000Z')
  expect(clearAfterTime('4h', null, at)).toBe('2026-10-25T16:00:00.000Z')
  const today = new Date(clearAfterTime('today', null, at) ?? '')
  expect([today.getHours(), today.getMinutes(), today.getSeconds(), today.getDate()]).toEqual([23, 59, 59, at.getDate()])
})

test('Save trims the text, sends no end for an empty status, and needs something to set or clear', () => {
  const none = currentStatus({ presence: 'idle' }, now)
  expect(customStatusBody(none, null, '  Lunch ', '30m', new Date(now))).toEqual({ presence: 'idle', emoji: null, text: 'Lunch', expires_at: '2026-10-25T12:30:00.000Z' })
  expect(customStatusBody(none, null, '   ', '30m', new Date(now))).toEqual({ presence: 'idle', emoji: null, text: null, expires_at: null })
  expect(canSaveCustomStatus(none, null, '  ')).toBe(false)
  expect(canSaveCustomStatus(none, '🌴', '')).toBe(true)
  expect(canSaveCustomStatus(currentStatus({ presence: 'idle', text: 'Lunch' }, now), null, '')).toBe(true)
})
