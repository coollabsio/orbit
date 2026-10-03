import { expect, test } from 'bun:test'
import { memberGroups } from './memberGroups'

const person = (id: string, name: string, role: 'Owner' | 'Admin' | 'Member' = 'Member') => ({ id, name, role })

test('online people group by role, offline people come last whatever their role', () => {
  const people = [person('u1', 'Zed'), person('u2', 'Ada', 'Owner'), person('u3', 'Bob', 'Admin'), person('u4', 'Cy', 'Admin'), person('u5', 'Amy')]
  const groups = memberGroups(people, new Set(['u1', 'u2', 'u3', 'u5']))
  expect(groups.map((group) => [group.label, group.people.map((item) => item.name)])).toEqual([
    ['Owner', ['Ada']],
    ['Admin', ['Bob']],
    ['Online', ['Amy', 'Zed']],
    ['Offline', ['Cy']],
  ])
  expect(memberGroups(people, new Set()).map((group) => group.label)).toEqual(['Offline'])
})
