import { expect, test } from 'bun:test'
import type { PresenceEntry } from '@/features/realtime/presence'
import { memberGroups } from './memberGroups'

const person = (id: string, name: string, role: 'Owner' | 'Admin' | 'Member' = 'Member') => ({ id, name, role })
const entry = (status: PresenceEntry['status']): PresenceEntry => ({ status, emoji: null, text: null, expiresAt: null })

test('online people group by role, offline people come last whatever their role', () => {
  const people = [person('u1', 'Zed'), person('u2', 'Ada', 'Owner'), person('u3', 'Bob', 'Admin'), person('u4', 'Cy', 'Admin'), person('u5', 'Amy')]
  const presence = new Map([['u1', entry('online')], ['u2', entry('online')], ['u3', entry('online')], ['u5', entry('online')]])
  expect(memberGroups(people, presence).map((group) => [group.label, group.people.map((item) => item.name)])).toEqual([
    ['Owner', ['Ada']],
    ['Admin', ['Bob']],
    ['Online', ['Amy', 'Zed']],
    ['Offline', ['Cy']],
  ])
  expect(memberGroups(people, new Map()).map((group) => group.label)).toEqual(['Offline'])
})

test('idle and do not disturb count as online', () => {
  const people = [person('u1', 'Zed'), person('u2', 'Ada', 'Owner'), person('u3', 'Bob')]
  const groups = memberGroups(people, new Map([['u1', entry('idle')], ['u2', entry('dnd')]]))
  expect(groups.map((group) => [group.label, group.online, group.people.map((item) => item.name)])).toEqual([
    ['Owner', true, ['Ada']],
    ['Online', true, ['Zed']],
    ['Offline', false, ['Bob']],
  ])
})
