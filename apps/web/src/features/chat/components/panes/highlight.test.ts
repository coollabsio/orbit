import { expect, test } from 'bun:test'
import { leadToFirstMatch, splitByRanges } from './highlight'

const shape = (segments: ReturnType<typeof splitByRanges>) => segments.map((segment) => (segment.match ? `[${segment.text}]` : segment.text)).join('')

test('text is cut at the ranges and joins back to the same text', () => {
  const text = 'ship the release today'
  const segments = splitByRanges(text, [[5, 8], [9, 16]])
  expect(shape(segments)).toBe('ship [the] [release] today')
  expect(segments.map((segment) => segment.text).join('')).toBe(text)
})

test('no ranges give one plain segment, and an empty text gives none', () => {
  expect(splitByRanges('hello', [])).toEqual([{ text: 'hello', match: false }])
  expect(splitByRanges('', [[0, 3]])).toEqual([])
})

test('a match at the start and at the end has no empty neighbours', () => {
  expect(splitByRanges('abc', [[0, 3]])).toEqual([{ text: 'abc', match: true }])
  expect(shape(splitByRanges('abcdef', [[0, 2], [4, 6]]))).toBe('[ab]cd[ef]')
})

test('unsorted, overlapping and touching ranges become one match', () => {
  expect(shape(splitByRanges('abcdefgh', [[4, 6], [1, 3], [2, 5]]))).toBe('a[bcdef]gh')
  expect(shape(splitByRanges('abcdef', [[1, 3], [3, 5]]))).toBe('a[bcde]f')
  expect(shape(splitByRanges('abcdef', [[1, 5], [2, 3]]))).toBe('a[bcde]f')
})

test('ranges outside the text are clamped or dropped', () => {
  expect(shape(splitByRanges('abc', [[-2, 1], [2, 9], [7, 9], [2, 2]]))).toBe('[a]b[c]')
})

test('a long lead before the first match is cut at a word start', () => {
  const segments = splitByRanges(`${'word '.repeat(30)}needle after`, [[150, 156]])
  const cut = leadToFirstMatch(segments, 12)
  expect(cut[0]).toEqual({ text: '…word word ', match: false })
  expect(cut.slice(1)).toEqual(segments.slice(1))
})

test('a short lead, a leading match and a text without a match stay as they are', () => {
  const short = splitByRanges('find the needle', [[9, 15]])
  expect(leadToFirstMatch(short)).toEqual(short)
  const leading = splitByRanges(`needle ${'x'.repeat(200)}`, [[0, 6]])
  expect(leadToFirstMatch(leading)).toEqual(leading)
  const plain = splitByRanges('x'.repeat(200), [])
  expect(leadToFirstMatch(plain)).toEqual(plain)
})
