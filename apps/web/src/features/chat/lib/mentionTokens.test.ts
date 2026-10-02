import { expect, test } from 'bun:test'
import { decodeMentions, encodeMentions, extractMentions } from './mentionTokens'

const people = [
  { id: 'u1', name: 'Ada' },
  { id: 'u2', name: 'Ada Lovelace' },
  { id: 'u3', name: 'Grace Hopper' },
]
const channels = [
  { id: 'c1', name: 'general' },
  { id: 'c2', name: 'general-help' },
]

test('names become id tokens, the longest name first', () => {
  expect(encodeMentions('hi @Ada Lovelace and @Ada, see #general-help or #general', people, channels)).toBe(
    'hi <@u2> and <@u1>, see <#c2> or <#c1>',
  )
})

test('matching ignores case and handles @channel and @here', () => {
  expect(encodeMentions('@grace hopper @CHANNEL @here', people, channels)).toBe('<@u3> <!channel> <!here>')
})

test('a name must start and end at a word boundary', () => {
  expect(encodeMentions('mail ada@Ada.example and @Adam and @hereby and x#general', people, channels)).toBe(
    'mail ada@Ada.example and @Adam and @hereby and x#general',
  )
  expect(encodeMentions('(@Ada) "@Ada"', people, channels)).toBe('(<@u1>) "<@u1>"')
})

test('code is not encoded', () => {
  expect(encodeMentions('`@Ada` and @Ada\n```\n@channel\n```', people, channels)).toBe('`@Ada` and <@u1>\n```\n@channel\n```')
})

test('tokens are shown by name; unknown ids do not leak', () => {
  expect(decodeMentions('<@u2> in <#c1>: <!channel> <!here> <@gone> <#gone>', people, channels)).toBe(
    '@Ada Lovelace in #general: @channel @here @unknown #unknown',
  )
})

test('encode and decode round-trip', () => {
  const text = 'ping @Grace Hopper in #general-help, @here'
  expect(decodeMentions(encodeMentions(text, people, channels), people, channels)).toBe(text)
})

test('extractMentions lists each user once and the two global mentions', () => {
  expect(extractMentions('<@u1> <@u2> <@u1> <!here> <#c1>')).toEqual({ userIds: ['u1', 'u2'], channel: false, here: true })
  expect(extractMentions('<!channel> plain @Ada')).toEqual({ userIds: [], channel: true, here: false })
})

test('a letter that is longer in lower case does not move the match', () => {
  const people = [{ id: 'u1', name: 'Ada' }, { id: 'u2', name: 'İpek' }]
  expect(encodeMentions('İyi günler @Ada', people, [])).toBe('İyi günler <@u1>')
  expect(encodeMentions('@İpek!', people, [])).toBe('<@u2>!')
})
