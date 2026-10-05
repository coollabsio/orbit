import { expect, test } from 'bun:test'
import { messageKey } from '../../lib/grouping'
import { testMessage } from '../../lib/testMessage'
import {
  displayLineCount,
  extractLinkUrls,
  firstLine,
  isJumboEmoji,
  isLongMessage,
  messagesAfter,
  quickReactions,
  standaloneLinkUrls,
  typingLabel,
} from './messageText'

test('a message collapses only when it is longer than 20 lines', () => {
  const lines = (count: number) => Array.from({ length: count }, (_, index) => `line ${index}`).join('\n')
  expect(isLongMessage(lines(20))).toBe(false)
  expect(isLongMessage(lines(21))).toBe(true)
  expect(isLongMessage('')).toBe(false)
})

test('a long source line counts as the lines it wraps to', () => {
  expect(displayLineCount('a'.repeat(90))).toBe(1)
  expect(displayLineCount('a'.repeat(91))).toBe(2)
  expect(displayLineCount('one\n\nthree')).toBe(3)
  expect(isLongMessage('a'.repeat(90 * 20 + 1))).toBe(true)
})

test('link URLs lose trailing punctuation and repeat once', () => {
  expect(extractLinkUrls('see https://orbit.test/tasks/ORB-1, and (https://orbit.test/docs/p1). https://orbit.test/tasks/ORB-1')).toEqual([
    'https://orbit.test/tasks/ORB-1',
    'https://orbit.test/docs/p1',
  ])
})

test('URLs in code and mention tokens are not link cards', () => {
  expect(extractLinkUrls('`https://a.test/x` <@u1> ```\nhttps://b.test/y\n``` https://c.test/z')).toEqual(['https://c.test/z'])
  expect(extractLinkUrls('no links <#c1>')).toEqual([])
})

test('link URLs stop at the limit', () => {
  expect(extractLinkUrls('https://a.test/1 https://a.test/2 https://a.test/3 https://a.test/4')).toHaveLength(3)
})

test('the first line skips blank lines and is cut with an ellipsis', () => {
  expect(firstLine('\n\n  hello  \nworld')).toBe('hello')
  expect(firstLine('abcdefghij', 5)).toBe('abcd…')
  expect(firstLine('')).toBe('')
})

test('quick reactions start with the ones the user used most, then the defaults', () => {
  const messages = [
    testMessage({ id: 'm1', reactions: [{ emoji: '🎉', userIds: ['me', 'u2'] }, { emoji: '👍', userIds: ['u2'] }] }),
    testMessage({ id: 'm2', reactions: [{ emoji: '🎉', userIds: ['me'] }, { emoji: '🚀', userIds: ['me'] }] }),
  ]
  expect(quickReactions(messages, 'me')).toEqual(['🎉', '🚀', '👍'])
  expect(quickReactions(messages, null)).toEqual(['👍', '👀', '😂'])
})

test('the typing line names up to two people', () => {
  expect(typingLabel([])).toBe('')
  expect(typingLabel(['Ada'])).toBe('Ada is typing')
  expect(typingLabel(['Ada', 'Grace'])).toBe('Ada and Grace are typing')
  expect(typingLabel(['Ada', 'Grace', 'Alan'])).toBe('Several people are typing')
})

test('new messages are the ones of other members after the last known message', () => {
  const messages = [
    testMessage({ id: 'm1' }),
    testMessage({ id: 'm2', authorId: 'u2' }),
    testMessage({ id: 'm3', authorId: 'me' }),
    testMessage({ id: 'm4', authorId: 'u2', kind: 'join' }),
    testMessage({ id: 'm5', authorId: 'u3' }),
  ]
  expect(messagesAfter(messages, 'm1', messageKey, 'me').map((message) => message.id)).toEqual(['m2', 'm5'])
  // The last known message was removed, or nothing was known: nothing counts as new.
  expect(messagesAfter(messages, 'gone', messageKey, 'me')).toEqual([])
  expect(messagesAfter(messages, null, messageKey, 'me')).toEqual([])
})

test('only a card URL that is a whole line is standalone', () => {
  const task = 'https://orbit.test/tasks/ENG-1'
  const page = 'https://orbit.test/docs/p1'
  expect(standaloneLinkUrls(task)).toEqual([task])
  expect(standaloneLinkUrls(`Look at this:\n${task}\nand ${page} too`)).toEqual([task])
  expect(standaloneLinkUrls(`- ${task}\n> ${page}\n${task}.`)).toEqual([])
  expect(standaloneLinkUrls(`\`\`\`\n${task}\n\`\`\``)).toEqual([])
  // the same URL in a sentence too: both stay, or the sentence would lose it
  expect(standaloneLinkUrls(`see ${task} now\n${task}`)).toEqual([])
  // past the card limit a URL has no card to stand in for it
  const many = [1, 2, 3, 4].map((number) => `https://orbit.test/tasks/ENG-${number}`)
  expect(standaloneLinkUrls(many.join('\n'))).toEqual(many.slice(0, 3))
})

test('a URL in angle brackets is not a link card', () => {
  const url = 'https://a.test/x'
  expect(extractLinkUrls(`<${url}> and https://b.test/y`)).toEqual(['https://b.test/y'])
  expect(standaloneLinkUrls(`<${url}>`)).toEqual([])
  // the bare one still has its card, and the card stands in for its line
  expect(extractLinkUrls(`<${url}> ${url}`)).toEqual([url])
  expect(standaloneLinkUrls(`see <${url}>\n${url}`)).toEqual([url])
  // an escaped bracket is text, so the URL after it is a bare one
  expect(extractLinkUrls(`\\<${url}>`)).toEqual([url])
})

test('a message of 1 to 30 emoji and nothing else is jumbo', () => {
  const custom = new Set(['parrot'])
  expect(isJumboEmoji('👍', custom)).toBe(true)
  expect(isJumboEmoji('👍 '.repeat(30).trim(), custom)).toBe(true)
  expect(isJumboEmoji('👍'.repeat(31), custom)).toBe(false)
  expect(isJumboEmoji('', custom)).toBe(false)
  expect(isJumboEmoji(' \n ', custom)).toBe(false)
  // a flag, a skin tone and a ZWJ family are one emoji each: 10 of each is 30
  expect(isJumboEmoji('🇮🇳👍🏽👨‍👩‍👧‍👦'.repeat(10), custom)).toBe(true)
  expect(isJumboEmoji('🇮🇳👍🏽👨‍👩‍👧‍👦'.repeat(10) + '👍', custom)).toBe(false)
  expect(isJumboEmoji('😀\n🎉  ❤️', custom)).toBe(true)
})

test('a known custom emoji counts for jumbo; other text does not', () => {
  const custom = new Set(['parrot'])
  expect(isJumboEmoji(':parrot:', custom)).toBe(true)
  expect(isJumboEmoji(':parrot::parrot: 👍', custom)).toBe(true)
  expect(isJumboEmoji(':parrot:'.repeat(29) + '👍👍', custom)).toBe(false)
  expect(isJumboEmoji(':nobody:', custom)).toBe(false)
  expect(isJumboEmoji('\\:parrot:', custom)).toBe(false)
  expect(isJumboEmoji('👍 nice', custom)).toBe(false)
  expect(isJumboEmoji('@Ada 👍', custom)).toBe(false)
  expect(isJumboEmoji('👍.', custom)).toBe(false)
})
