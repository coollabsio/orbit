import { expect, test } from 'bun:test'
import { quoteOf, wordlessContentOf } from './quote'
import { testMessage } from './testMessage'

const media = (kind: 'gif' | 'sticker') => ({ kind, url: 'https://static.klipy.com/a.gif', width: 320, height: 240, title: 'Wave' })

test('a quote says whether its message is a sticker or a GIF, whichever kind of sticker it is', () => {
  const sticker = { id: 's1', name: 'Party Parrot', url: '/stickers/s1' }
  expect(quoteOf(testMessage({ id: 'm1', body: '', stickerId: 's1', sticker }))).toMatchObject({ sticker: true, gif: false })
  expect(quoteOf(testMessage({ id: 'm2', body: '', media: media('sticker') }))).toMatchObject({ sticker: true, gif: false })
  expect(quoteOf(testMessage({ id: 'm3', body: '', media: media('gif') }))).toMatchObject({ sticker: false, gif: true })
  expect(quoteOf(testMessage({ id: 'm4' }))).toMatchObject({ sticker: false, gif: false })
})

test('a row cached before media existed has neither flag', () => {
  expect(wordlessContentOf({ stickerId: null, media: undefined as unknown as null })).toEqual({ sticker: false, gif: false })
})
