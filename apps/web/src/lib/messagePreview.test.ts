import { expect, test } from 'bun:test'
import { extractPreview, wordlessPreview } from './messagePreview'

test('a preview is the first line with text, without its markdown', () => {
  expect(extractPreview('\n```\n## **Hello** _there_ `code` [docs](https://x.test/a)\nmore')).toBe('Hello there code docs')
  expect(extractPreview('> - quoted')).toBe('quoted')
  expect(extractPreview('')).toBe('')
})

test('a preview shows what the message shows, not the escapes of its source', () => {
  expect(extractPreview('\\:D and \\*stars\\*')).toBe(':D and *stars*')
  expect(extractPreview('see <https://x.test/a?b=1> now')).toBe('see https://x.test/a?b=1 now')
  // no project list here: the backslash of an escaped task identifier stays, as a backslash before any letter does
  expect(extractPreview('\\ENG-12 C:\\dir')).toBe('\\ENG-12 C:\\dir')
  // mentions and plain angle brackets are as before
  expect(extractPreview('@Ada Lovelace in #general: a < b > c')).toBe('@Ada Lovelace in #general: a < b > c')
})

test('a message without text previews as its sticker, else as a file', () => {
  expect(extractPreview('') || wordlessPreview(true)).toBe('Sticker')
  expect(extractPreview('') || wordlessPreview(false)).toBe('Sent a file')
})
