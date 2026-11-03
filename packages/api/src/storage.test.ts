import { expect, test } from 'bun:test'
import { canDeleteStorageFile, storageFileLabel, storageFilesPage } from './storage'

const id = '01890000-0000-7000-8000-000000000000'

test('a stored file names each place once and hides private names', () => {
  expect(storageFileLabel({
    id,
    byte_size: 1,
    name: 'demo.mp4',
    uses: [{ kind: 'task', title: 'Launch' }, { kind: 'page', title: '' }, { kind: 'channel', title: 'general' }, { kind: 'private' }, { kind: 'private' }],
  })).toEqual({ name: 'demo.mp4', where: 'Task “Launch”, Page “Untitled”, #general, Private page or chat' })
  expect(storageFileLabel({ id, byte_size: 1, name: null, uses: [{ kind: 'private' }] })).toEqual({ name: 'Private file', where: 'Private page or chat' })
  expect(storageFileLabel({ id, byte_size: 1, name: null, uses: [] })).toEqual({ name: 'Deleted file', where: 'Deleted · stops counting within a day' })
})

test('files of emoji and stickers are deleted on their own pages', () => {
  expect(canDeleteStorageFile({ id, byte_size: 1, name: 'a.png', uses: [{ kind: 'task', title: 'A' }, { kind: 'page', title: 'B' }] })).toBe(true)
  expect(canDeleteStorageFile({ id, byte_size: 1, name: 'party', uses: [{ kind: 'emoji' }] })).toBe(false)
  expect(canDeleteStorageFile({ id, byte_size: 1, name: 'cat', uses: [{ kind: 'task', title: 'A' }, { kind: 'sticker' }] })).toBe(false)
})

test('pages of the largest files', () => {
  expect(storageFilesPage(0, 10, 34)).toEqual({ label: '1–10 of 34', previous: null, next: 10 })
  expect(storageFilesPage(10, 10, 34)).toEqual({ label: '11–20 of 34', previous: 0, next: 20 })
  expect(storageFilesPage(30, 4, 34)).toEqual({ label: '31–34 of 34', previous: 20, next: null })
  expect(storageFilesPage(0, 7, 7)).toEqual({ label: '1–7 of 7', previous: null, next: null })
})
