import { describe, expect, test } from 'bun:test'
import { cardFace, galleryFields, galleryOf } from './gallery'
import { row } from './testing'
import type { DatabaseProperty, DatabaseRow, Property, ViewConfig } from './types'
import { parseProperty, parseRow } from './values'

const properties: Property[] = (
  [
    { id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 },
    { id: 'p-files', name: 'Photos', type: 'files', config: {}, position: 1 },
    { id: 'p-text', name: 'Notes', type: 'text', config: {}, position: 2 },
  ] satisfies DatabaseProperty[]
).map(parseProperty)

const gallery = (config: Parameters<typeof galleryFields>[0]) => galleryOf(galleryFields(config, properties) as ViewConfig)
const card = (values: Record<string, unknown>, preview?: DatabaseRow['preview']) => parseRow(row('r1', 0, 'Trip', values, preview ? { preview } : {}), properties)

describe('galleryFields and galleryOf', () => {
  test('an empty config shows the body on a medium card that fills the image', () => {
    expect(galleryFields({}, properties)).toEqual({})
    expect(gallery({})).toEqual({ preview: 'content', size: 'medium', fitImage: false })
  })

  test('non-default preview, size and fit are kept', () => {
    const config = { card_preview: { files: 'p-files' }, card_size: 'large' as const, fit_image: true }
    expect(galleryFields(config, properties)).toEqual({ card_preview: { files: 'p-files' }, card_size: 'large', fit_image: true })
    expect(gallery({ card_preview: 'cover', card_size: 'small' })).toEqual({ preview: 'cover', size: 'small', fitImage: false })
    expect(gallery({ card_preview: 'none' })).toEqual({ preview: 'none', size: 'medium', fitImage: false })
  })

  test('a files preview of a deleted or non-files property shows the body', () => {
    expect(galleryFields({ card_preview: { files: 'p-gone' }, card_size: 'medium' }, properties)).toEqual({})
    expect(gallery({ card_preview: { files: 'p-text' } })).toEqual({ preview: 'content', size: 'medium', fitImage: false })
  })
})

describe('cardFace', () => {
  test('cover: the row cover with its position, else an empty box', () => {
    const cover = gallery({ card_preview: 'cover' })
    expect(cardFace(card({}, { cover_url: '/covers/a.png', cover_position: '50% 20%' }), cover, properties, 'ws')).toEqual({ kind: 'image', url: '/covers/a.png', position: '50% 20%' })
    expect(cardFace(card({}, { text: 'Body' }), cover, properties, 'ws')).toEqual({ kind: 'empty' })
  })

  test('content: the body image first, then the body text, else an empty box', () => {
    const content = gallery({})
    expect(cardFace(card({}, { image_url: '/img/b.png', text: 'Body' }), content, properties, 'ws')).toEqual({ kind: 'image', url: '/img/b.png', position: null })
    expect(cardFace(card({}, { text: 'Body' }), content, properties, 'ws')).toEqual({ kind: 'text', text: 'Body' })
    expect(cardFace(card({}), content, properties, 'ws')).toEqual({ kind: 'empty' })
  })

  test('files: the first image file of the property as a row file path, else an empty box', () => {
    const files = gallery({ card_preview: { files: 'p-files' } })
    const values = { 'p-files': [{ file_id: 'f-doc', name: 'notes.pdf' }, { file_id: 'f-img', name: 'Beach.JPG' }] }
    expect(cardFace(card(values), files, properties, 'ws')).toEqual({ kind: 'image', url: '/api/v1/workspaces/ws/pages/r1/files/f-img', position: null })
    expect(cardFace(card({ 'p-files': [{ file_id: 'f-doc', name: 'notes.pdf' }] }), files, properties, 'ws')).toEqual({ kind: 'empty' })
  })

  test('none: no preview area', () => {
    expect([cardFace(card({}, { image_url: '/img/b.png' }), gallery({ card_preview: 'none' }), properties, 'ws'), cardFace(card({}, { image_url: '/img/b.png' }), gallery({}), properties, 'ws')?.kind]).toEqual([null, 'image'])
  })
})
