import { expect, test } from 'bun:test'
import { normalizeEmoji, splitEmoji, twemojiUrl } from './twemoji'

test('text is cut into text and emoji, in order', () => {
  expect(splitEmoji('Done 🎉 thanks 👍')).toEqual([
    { text: 'Done ', url: null },
    { text: '🎉', url: '/assets/twemoji-15.0.0/1f389.svg' },
    { text: ' thanks ', url: null },
    { text: '👍', url: '/assets/twemoji-15.0.0/1f44d.svg' },
  ])
})

test('text without emoji stays one segment, and empty text has none', () => {
  expect(splitEmoji('No emoji here: 100%')).toEqual([{ text: 'No emoji here: 100%', url: null }])
  expect(splitEmoji('café — naïve')).toEqual([{ text: 'café — naïve', url: null }])
  expect(splitEmoji('')).toEqual([])
})

test('the file name drops the variation selector, but keeps it in a joined sequence', () => {
  // ❤️ is U+2764 U+FE0F: the file is 2764.svg
  expect(twemojiUrl('❤️')).toBe('/assets/twemoji-15.0.0/2764.svg')
  // 🏳️‍🌈 keeps its selector because it has a zero-width joiner
  expect(twemojiUrl('🏳️‍🌈')).toBe('/assets/twemoji-15.0.0/1f3f3-fe0f-200d-1f308.svg')
  // a skin tone is part of the name
  expect(twemojiUrl('👍🏽')).toBe('/assets/twemoji-15.0.0/1f44d-1f3fd.svg')
})

test('only a value that is exactly one emoji has an image', () => {
  expect(twemojiUrl('🎉')).toBe('/assets/twemoji-15.0.0/1f389.svg')
  expect(twemojiUrl('🎉🎉')).toBeNull()
  expect(twemojiUrl('ok 🎉')).toBeNull()
  expect(twemojiUrl('A')).toBeNull()
})

test('a selector that the parser leaves apart belongs to its emoji', () => {
  // The picker data has 👍 as U+1F44D U+FE0F.
  expect(twemojiUrl('👍️')).toBe('/assets/twemoji-15.0.0/1f44d.svg')
  expect(splitEmoji('ok 👍️')).toEqual([
    { text: 'ok ', url: null },
    { text: '👍️', url: '/assets/twemoji-15.0.0/1f44d.svg' },
  ])
  expect(normalizeEmoji('👍️')).toBe('👍')
  expect(normalizeEmoji('❤️')).toBe('❤️')
  expect(normalizeEmoji('🏳️‍🌈')).toBe('🏳️‍🌈')
})

test('a skin tone after an emoji that has no image stays text', () => {
  expect(splitEmoji('ok ✌🏽 done')).toEqual([{ text: 'ok ✌🏽 done', url: null }])
  expect(twemojiUrl('🏽')).toBe('/assets/twemoji-15.0.0/1f3fd.svg')
})
