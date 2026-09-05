import { expect, test } from 'bun:test'
import { commentUploadMode } from './commentUpload'

test('attachment-only comments use their dedicated generated endpoint path', () => {
  expect(commentUploadMode('', 1)).toBe('attachment-only')
  expect(commentUploadMode('  ', 2)).toBe('attachment-only')
  expect(commentUploadMode('A note', 1)).toBe('text')
  expect(commentUploadMode('', 0)).toBe('invalid')
})
