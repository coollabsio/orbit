import { expect, test } from 'bun:test'
import { STICKER_MAX_BYTES, searchStickers, stickerNameFromFile, stickerProblem } from './customSticker'

test('a file name gives a name: no extension, trimmed, at most 30 characters', () => {
  expect(stickerNameFromFile('Party Parrot.GIF')).toBe('Party Parrot')
  expect(stickerNameFromFile('  héllo wörld.final.png')).toBe('héllo wörld.final')
  expect(stickerNameFromFile(`${'a'.repeat(29)} b.png`)).toBe('a'.repeat(29))
  expect(stickerNameFromFile('.png')).toBe('')
})

test('a new sticker is checked before the upload: type, size, name, and a name in use', () => {
  const png = { type: 'image/png', size: 1000 }
  expect(stickerProblem(png, 'Party Parrot', [])).toBeNull()
  expect(stickerProblem({ type: 'image/gif', size: STICKER_MAX_BYTES }, 'ok', [])).toBeNull()
  expect(stickerProblem(png, '  ok  ', [])).toBeNull()
  expect(stickerProblem({ type: 'image/svg+xml', size: 1000 }, 'party', [])).toContain('PNG')
  expect(stickerProblem({ type: 'image/png', size: STICKER_MAX_BYTES + 1 }, 'party', [])).toContain('512 KB')
  for (const name of ['', 'a', ' a ', 'a'.repeat(31), 'tab\there']) {
    expect(stickerProblem(png, name, [])).toContain('2 to 30')
  }
  expect(stickerProblem(png, ' party parrot ', ['Party Parrot'])).toContain('exists already')
  expect(stickerProblem(png, 'Party', ['Party Parrot'])).toBeNull()
})

test('the search keeps the stickers whose name contains it, without regard to case', () => {
  const stickers = [{ name: 'Party Parrot' }, { name: 'Sad cat' }]
  expect(searchStickers(stickers, ' PARROT ')).toEqual([{ name: 'Party Parrot' }])
  expect(searchStickers(stickers, '')).toEqual(stickers)
  expect(searchStickers(stickers, 'dog')).toEqual([])
})
