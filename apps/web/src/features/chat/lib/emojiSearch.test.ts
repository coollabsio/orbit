import { expect, test } from 'bun:test'
import { buildEmojiIndex, emojiNamed, searchEmojiIndex } from './emojiSearch'

const index = buildEmojiIndex(
  [
    { hexcode: '1F602', emoji: '😂', version: 0.6 },
    { hexcode: '1F639', emoji: '😹', version: 0.6 },
    { hexcode: '1F44D', emoji: '👍', version: 0.6 },
    { hexcode: '1FAE0', emoji: '🫠', version: 14 },
    { hexcode: '1FAE8', emoji: '🫨', version: 15 },
    // Newer than the images: left out.
    { hexcode: '1FAC6', emoji: '🫆', version: 18 },
    { hexcode: '1F600', emoji: '😀', version: 1 },
  ],
  { '1F602': 'joy', '1F639': 'joy_cat', '1F44D': ['+1', 'thumbsup'], '1FAE0': 'melting_face', '1FAE8': 'shaking_face', '1FAC6': 'fingerprint' },
)

test('the index has one entry for each name, and leaves out emoji that have no image or no name', () => {
  expect(index.map((entry) => entry.name)).toEqual(['joy', 'joy_cat', '+1', 'thumbsup', 'melting_face', 'shaking_face'])
})

test('names that start with the query come before names that contain it', () => {
  expect(searchEmojiIndex(index, 'fa').map((entry) => entry.name)).toEqual(['melting_face', 'shaking_face'])
  expect(searchEmojiIndex(index, 'jo').map((entry) => entry.emoji)).toEqual(['😂', '😹'])
  expect(searchEmojiIndex(index, 'cat').map((entry) => entry.emoji)).toEqual(['😹'])
})

test('an emoji with two names shows once, and the limit holds', () => {
  expect(searchEmojiIndex(index, 'u').map((entry) => entry.emoji)).toEqual(['👍'])
  expect(searchEmojiIndex(index, 'JO', 1)).toEqual([{ emoji: '😂', name: 'joy' }])
})

test('a full name gives its emoji', () => {
  expect(emojiNamed(index, 'thumbsup')).toBe('👍')
  expect(emojiNamed(index, '+1')).toBe('👍')
  expect(emojiNamed(index, 'JOY')).toBe('😂')
  expect(emojiNamed(index, 'jo')).toBeNull()
})
