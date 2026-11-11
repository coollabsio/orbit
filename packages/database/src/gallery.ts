import { firstImageFile, rowFileUrl } from './fields'
import type { CardPreview, CardSize, DatabaseViewConfig, Property, Row, ViewConfig } from './types'
import { cellOf } from './values'

/** Card widths on the web, by card size (spec P3 16.1). */
export const CARD_WIDTH: Record<CardSize, number> = { small: 180, medium: 260, large: 340 }

export const CARD_SIZES = ['small', 'medium', 'large'] as const satisfies readonly CardSize[]

/** A gallery view's fields with defaults filled in. */
export type Gallery = { preview: CardPreview; size: CardSize; fitImage: boolean }

/** Like the server's `gallery::repair`: a files preview whose property is gone or holds no files shows the body. */
export function galleryFields(config: DatabaseViewConfig, properties: readonly Property[]): Partial<ViewConfig> {
  const preview = config.card_preview
  const files = typeof preview === 'object' && preview !== null ? properties.find((property) => property.id === preview.files) : undefined
  const kept = typeof preview === 'object' ? (files?.type === 'files' ? preview : 'content') : preview
  return {
    ...(kept && kept !== 'content' ? { card_preview: kept } : {}),
    ...(config.card_size && config.card_size !== 'medium' ? { card_size: config.card_size } : {}),
    ...(config.fit_image ? { fit_image: true } : {}),
  }
}

export function galleryOf(config: ViewConfig): Gallery {
  return { preview: config.card_preview ?? 'content', size: config.card_size ?? 'medium', fitImage: config.fit_image ?? false }
}

/** What a card shows above its title: an image, the start of the body text, or an empty box. `null`: no preview area. */
export type CardFace = { kind: 'image'; url: string; position: string | null } | { kind: 'text'; text: string } | { kind: 'empty' }

/** The card preview of a row; the server sends `row.preview` when a view it reads is a gallery. Image URLs are same-origin paths. */
export function cardFace(row: Row, gallery: Gallery, properties: readonly Property[], workspaceId: string): CardFace | null {
  const preview = gallery.preview
  if (preview === 'none') return null
  if (preview === 'cover') {
    const url = row.preview?.cover_url
    return url ? { kind: 'image', url, position: row.preview?.cover_position ?? null } : { kind: 'empty' }
  }
  if (preview === 'content') {
    if (row.preview?.image_url) return { kind: 'image', url: row.preview.image_url, position: null }
    return row.preview?.text ? { kind: 'text', text: row.preview.text } : { kind: 'empty' }
  }
  const property = properties.find((candidate) => candidate.id === preview.files)
  const file = property ? firstImageFile(cellOf(row, property)) : null
  return file ? { kind: 'image', url: rowFileUrl(workspaceId, row.id, file.file_id), position: null } : { kind: 'empty' }
}
