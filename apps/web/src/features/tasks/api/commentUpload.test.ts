import { expect, test } from 'bun:test'
import { EMPTY_DOCUMENT } from '../../../components/editor/document'
import { commentUploadMode } from './commentUpload'

const text = (value: string) => ({ type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: value }] }] })

test('attachment-only comments use their dedicated generated endpoint path', () => {
  expect(commentUploadMode(EMPTY_DOCUMENT, 1)).toBe('attachment-only')
  expect(commentUploadMode(text('  '), 2)).toBe('attachment-only')
  expect(commentUploadMode(text('A note'), 1)).toBe('text')
  expect(commentUploadMode(EMPTY_DOCUMENT, 0)).toBe('invalid')
})

test('a comment holding only a mention is text, not empty', () => {
  const mention = { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'mention', attrs: { id: 'u1', label: 'Ada' } }] }] }

  expect(commentUploadMode(mention, 0)).toBe('text')
})
