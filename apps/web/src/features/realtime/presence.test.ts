import { afterEach, expect, test } from 'bun:test'
import { customStatusOf, type PresenceEntry, presenceOf, replacePresence, resetPresence, setPresence, usePresence } from './presence'
import { renderHook } from '@testing-library/react'

const entry = (status: PresenceEntry['status'], text: string | null = null, expiresAt: string | null = null): PresenceEntry => ({ status, emoji: text ? '🍜' : null, text, expiresAt })
const current = () => {
  const hook = renderHook(() => usePresence())
  const map = hook.result.current
  hook.unmount()
  return map
}

afterEach(resetPresence)

test('a member is set, changed and taken out; somebody who is not in the map is offline', () => {
  setPresence('u1', entry('online'))
  setPresence('u2', entry('dnd', 'Focus'))
  const before = current()
  expect(presenceOf(before, 'u1')).toEqual({ status: 'online', emoji: null, text: null })
  expect(presenceOf(before, 'u2')).toEqual({ status: 'dnd', emoji: '🍜', text: 'Focus' })
  expect(presenceOf(before, 'u9')).toEqual({ status: 'offline', emoji: null, text: null })
  expect(presenceOf(before, null).status).toBe('offline')

  setPresence('u1', entry('idle'))
  setPresence('u2', null)
  const after = current()
  // a change is a new map, so subscribers see it
  expect(after).not.toBe(before)
  expect(presenceOf(after, 'u1').status).toBe('idle')
  expect(after.has('u2')).toBe(false)

  // offline for somebody who was not there changes nothing
  setPresence('u9', null)
  expect(current()).toBe(after)
})

test('the list of a hello replaces everything', () => {
  setPresence('u1', entry('online'))
  replacePresence(new Map([['u2', entry('idle')]]))
  expect([...current().keys()]).toEqual(['u2'])
})

test('a custom status that has ended is left out, the presence stays', () => {
  const past = new Date(Date.now() - 1000).toISOString()
  replacePresence(new Map([['u1', entry('dnd', 'Lunch', past)], ['u2', entry('online', 'Lunch', 'not a date')]]))
  expect(current().get('u1')).toEqual({ status: 'dnd', emoji: null, text: null, expiresAt: null })
  expect(current().get('u2')).toEqual({ status: 'online', emoji: null, text: null, expiresAt: null })
})

test('a custom status goes away when its time comes, without a signal', async () => {
  setPresence('u1', entry('online', 'Lunch', new Date(Date.now() + 40).toISOString()))
  setPresence('u2', entry('idle', 'Away', new Date(Date.now() + 60_000).toISOString()))
  expect(presenceOf(current(), 'u1').text).toBe('Lunch')
  await new Promise((resolve) => setTimeout(resolve, 80))
  expect(presenceOf(current(), 'u1')).toEqual({ status: 'online', emoji: null, text: null })
  expect(presenceOf(current(), 'u2').text).toBe('Away')
})

test('customStatusOf hides an expired status and treats empty strings as absent', () => {
  const now = Date.parse('2026-10-03T12:00:00Z')
  expect(customStatusOf({ emoji: '🍜', text: 'Lunch', expiresAt: null }, now)).toEqual({ emoji: '🍜', text: 'Lunch' })
  expect(customStatusOf({ emoji: '🍜', text: 'Lunch', expiresAt: '2026-10-03T12:00:01Z' }, now)).toEqual({ emoji: '🍜', text: 'Lunch' })
  expect(customStatusOf({ emoji: '🍜', text: 'Lunch', expiresAt: '2026-10-03T12:00:00Z' }, now)).toEqual({ emoji: null, text: null })
  expect(customStatusOf({ emoji: '', text: '', expiresAt: null }, now)).toEqual({ emoji: null, text: null })
})
