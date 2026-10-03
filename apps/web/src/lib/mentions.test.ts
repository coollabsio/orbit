import { expect, test } from 'bun:test'
import { buildMentionTokens, mentionedUserId } from './mentions'

const people = [
  { id: 'u1', name: 'Ada Lovelace', handle: 'ada' },
  { id: 'u2', name: 'Sam Lee', handle: 'sam' },
  { id: 'u3', name: 'Sam Lee', handle: 'slee' },
  { id: 'u4', name: 'here', handle: 'grace' },
]
const tokens = buildMentionTokens(people, [{ id: 'c1', name: 'ada' }])

test('a mention names its member by name or handle, without regard to case', () => {
  expect(mentionedUserId('Ada Lovelace', tokens)).toBe('u1')
  expect(mentionedUserId('ada lovelace', tokens)).toBe('u1')
  expect(mentionedUserId('ADA', tokens)).toBe('u1')
  expect(mentionedUserId('slee', tokens)).toBe('u3')
})

test('a mention that does not name one member opens no profile', () => {
  // two members with the name
  expect(mentionedUserId('Sam Lee', tokens)).toBeNull()
  // `@here` is the notification, also when a member has it as a name
  expect(mentionedUserId('here', tokens)).toBeNull()
  expect(mentionedUserId('channel', tokens)).toBeNull()
  expect(mentionedUserId('nobody', tokens)).toBeNull()
  // tokens without a member id (built by another caller)
  expect(mentionedUserId('Ada', [{ label: 'Ada', color: '#fff', kind: 'user' }])).toBeNull()
})
