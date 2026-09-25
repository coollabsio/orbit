import { expect, test } from 'bun:test'
import { highlightSegments } from './searchHighlights'

test('splits text into plain and matched segments at UTF-16 ranges', () => {
  expect(highlightSegments('…a Über 🙂 plans', [{ start: 3, end: 7 }, { start: 11, end: 15 }])).toEqual([
    { text: '…a ', match: false },
    { text: 'Über', match: true },
    { text: ' 🙂 ', match: false },
    { text: 'plan', match: true },
    { text: 's', match: false },
  ])
})

test('no ranges keep the whole text; bad ranges are clamped or skipped', () => {
  expect(highlightSegments('plain', [])).toEqual([{ text: 'plain', match: false }])
  expect(highlightSegments('plain', undefined)).toEqual([{ text: 'plain', match: false }])
  expect(highlightSegments('', [])).toEqual([{ text: '', match: false }])
  expect(
    highlightSegments('abcdef', [
      { start: 4, end: 99 },
      { start: 0, end: 2 },
      { start: 1, end: 3 },
      { start: 5, end: 5 },
      { start: 50, end: 60 },
    ]),
  ).toEqual([
    { text: 'ab', match: true },
    { text: 'c', match: true },
    { text: 'd', match: false },
    { text: 'ef', match: true },
  ])
})
