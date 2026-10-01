import { expect, test } from 'bun:test'
import { ChatError } from '@/features/chat/api/types'
import { canAddPeople, canManageChannel, channelNameFinal, channelNameInput, chatErrorMessage, matchPeople, pickablePeople } from './channelLib'

test('a channel name is lowercase and spaces become hyphens while it is typed', () => {
  expect(channelNameInput('Design Team')).toBe('design-team')
  expect(channelNameInput('design ')).toBe('design-')
  expect(channelNameInput('a  -  b')).toBe('a-b')
  expect(channelNameInput('#general')).toBe('general')
  expect(channelNameInput(' lead')).toBe('lead')
})

test('a channel name is cut at 80 characters', () => {
  expect(channelNameInput('x'.repeat(100))).toHaveLength(80)
})

test('the sent name has no hyphen at the end', () => {
  expect(channelNameFinal('design ')).toBe('design')
  expect(channelNameFinal('  ')).toBe('')
})

const channel = { kind: 'private', createdBy: 'u1', isMember: true, isDefault: false, archived: false } as const

test('the creator, owners and admins manage a channel; nobody manages a DM', () => {
  expect(canManageChannel(channel, { id: 'u1', role: 'Member' })).toBe(true)
  expect(canManageChannel(channel, { id: 'u2', role: 'Member' })).toBe(false)
  expect(canManageChannel(channel, { id: 'u2', role: 'Admin' })).toBe(true)
  expect(canManageChannel(channel, { id: 'u2', role: 'Owner' })).toBe(true)
  expect(canManageChannel(channel, undefined)).toBe(false)
  expect(canManageChannel({ kind: 'dm', createdBy: 'u1' }, { id: 'u1', role: 'Owner' })).toBe(false)
})

test('add people: managers in a private channel, members in a public one, never in #general or a DM', () => {
  const member = { id: 'u2', role: 'Member' } as const
  expect(canAddPeople(channel, member)).toBe(false)
  expect(canAddPeople(channel, { id: 'u1', role: 'Member' })).toBe(true)
  expect(canAddPeople({ ...channel, kind: 'public' }, member)).toBe(true)
  expect(canAddPeople({ ...channel, kind: 'public', isMember: false }, member)).toBe(false)
  expect(canAddPeople({ ...channel, kind: 'public', isDefault: true }, { id: 'u2', role: 'Owner' })).toBe(false)
  expect(canAddPeople({ ...channel, kind: 'public', archived: true }, member)).toBe(false)
  expect(canAddPeople({ ...channel, kind: 'dm' }, { id: 'u1', role: 'Owner' })).toBe(false)
})

const people = [
  { id: 'u1', name: 'Me', handle: 'me', email: 'me@orbit.test', role: 'Owner' },
  { id: 'u2', name: 'Zoe Park', handle: 'zoe', email: 'zoe@orbit.test', role: 'Member' },
  { id: 'u3', name: 'Ada Lovelace', handle: 'countess', email: 'ada@orbit.test', role: 'Member' },
  { id: 'u4', name: 'Gone', handle: 'gone', email: 'gone@orbit.test', role: 'Member', suspended: true },
] as const

test('a picker offers nobody suspended, not the current user and not the excluded ones, by name', () => {
  expect(pickablePeople(people, 'u1').map((person) => person.id)).toEqual(['u3', 'u2'])
  expect(pickablePeople(people, 'u1', ['u3']).map((person) => person.id)).toEqual(['u2'])
})

test('people match by name, handle or email', () => {
  expect(matchPeople(people, 'LOVE').map((person) => person.id)).toEqual(['u3'])
  expect(matchPeople(people, '@countess').map((person) => person.id)).toEqual(['u3'])
  expect(matchPeople(people, 'zoe@').map((person) => person.id)).toEqual(['u2'])
  expect(matchPeople(people, ' ')).toHaveLength(4)
})

test('a chat error becomes the sentence of the action, a general one, or the fallback', () => {
  const fallback = 'The channel was not created. Try again.'
  const taken = 'A channel with this name already exists.'
  expect(chatErrorMessage(new ChatError('conflict', 'x'), fallback, { conflict: taken })).toBe(taken)
  expect(chatErrorMessage(new ChatError('forbidden', 'x'), fallback)).toBe('You do not have permission to do this.')
  expect(chatErrorMessage(new ChatError('not_found', 'x'), fallback)).toBe(fallback)
  expect(chatErrorMessage(new Error('network'), fallback)).toBe(fallback)
})
