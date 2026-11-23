import { describe, expect, test } from 'bun:test'

import { parseSwipes, swipeStep } from './mail-swipe'

const thread = { folder: 'inbox' as const, unread: true, starred: false }

describe('parseSwipes', () => {
  test('defaults to archive on the left and reply all on the right', () => {
    expect(parseSwipes(null)).toEqual({ left: 'archive', right: 'reply_all' })
  })
  test('keeps a stored choice and replaces an unknown or broken one', () => {
    expect(parseSwipes('{"left":"trash","right":"fly"}')).toEqual({
      left: 'trash',
      right: 'reply_all',
    })
    expect(parseSwipes('not json')).toEqual({
      left: 'archive',
      right: 'reply_all',
    })
    expect(parseSwipes('null')).toEqual({
      left: 'archive',
      right: 'reply_all',
    })
  })
})

describe('swipeStep', () => {
  test('archive follows the folder', () => {
    expect(swipeStep('archive', thread)).toMatchObject({
      label: 'Archive',
      outcome: { change: { folder: 'archive' } },
      leaves: true,
    })
    expect(swipeStep('archive', { ...thread, folder: 'archive' })).toMatchObject({
      label: 'Move to inbox',
      outcome: { change: { folder: 'inbox' } },
    })
    expect(swipeStep('archive', { ...thread, folder: 'trash' })).toBeNull()
  })
  test('trash restores in the trash, and spam does nothing there', () => {
    expect(swipeStep('trash', { ...thread, folder: 'trash' })).toMatchObject({
      label: 'Restore',
      outcome: { change: { restore: true } },
    })
    expect(swipeStep('spam', { ...thread, folder: 'trash' })).toBeNull()
  })
  test('toggles read and star, and replies without a change', () => {
    expect(swipeStep('read', thread)).toMatchObject({
      outcome: { change: { read: true } },
      leaves: false,
    })
    expect(swipeStep('star', thread)).toMatchObject({
      label: 'Star',
      outcome: { change: { starred: true } },
    })
    expect(swipeStep('reply_all', thread)).toMatchObject({
      label: 'Reply all',
      outcome: { reply: 'reply_all' },
      leaves: false,
    })
    expect(swipeStep('none', thread)).toBeNull()
  })
})
