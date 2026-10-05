import { expect, test } from 'bun:test'
import { EMOJI_MAX_BYTES, customEmojiName, emojiNameFromFile, emojiProblem } from './customEmoji'

test('a file name gives a name the server takes', () => {
  expect(emojiNameFromFile('Party Parrot.GIF')).toBe('party_parrot')
  expect(emojiNameFromFile('--héllo--wörld.final.png')).toBe('h_llo_w_rld_final')
  expect(emojiNameFromFile(`${'a'.repeat(40)}.png`)).toHaveLength(32)
  expect(emojiNameFromFile('.png')).toBe('')
})

test('only an exact :name: is a custom emoji value', () => {
  expect(customEmojiName(':party_parrot:')).toBe('party_parrot')
  expect(customEmojiName(':a:')).toBeNull()
  expect(customEmojiName(':Party:')).toBeNull()
  expect(customEmojiName(':+1:')).toBeNull()
  expect(customEmojiName('👍')).toBeNull()
  expect(customEmojiName('a :party: b')).toBeNull()
})

test('a new emoji is checked before the upload: type, size, name, and a name in use', () => {
  const png = { type: 'image/png', size: 1000 }
  const free = () => false
  expect(emojiProblem(png, 'party_parrot', free)).toBeNull()
  expect(emojiProblem({ type: 'image/gif', size: EMOJI_MAX_BYTES }, 'ok', free)).toBeNull()
  expect(emojiProblem({ type: 'image/svg+xml', size: 1000 }, 'party', free)).toContain('PNG')
  expect(emojiProblem({ type: 'image/png', size: EMOJI_MAX_BYTES + 1 }, 'party', free)).toContain('256 KB')
  for (const name of ['', 'a', 'Party', 'party-parrot', 'party parrot', 'a'.repeat(33)]) {
    expect(emojiProblem(png, name, free)).toContain('2 to 32')
  }
  expect(emojiProblem(png, 'joy', (name) => name === 'joy')).toContain(':joy: exists already')
})
