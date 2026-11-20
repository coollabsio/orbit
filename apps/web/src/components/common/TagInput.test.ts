import { expect, test } from 'bun:test'
import { addTags } from './TagInput'

test('addTags splits pasted text on commas and spaces and skips duplicates and blanks', () => {
  expect(addTags(['mx1.example.com'], ' mx2.example.com, mx1.example.com,,\nmx3.example.com ')).toEqual(['mx1.example.com', 'mx2.example.com', 'mx3.example.com'])
  expect(addTags(['a'], '  ')).toEqual(['a'])
})
