import type { StorageFile, StorageFileUse } from './generated/types.gen'

/** The name and the "where" line of a file in the Storage card's largest files. */
export function storageFileLabel(file: StorageFile): { name: string; where: string } {
  if (file.uses.length === 0) return { name: file.name ?? 'Deleted file', where: 'Deleted · stops counting within a day' }
  const places = [...new Set(file.uses.map(usePlace))]
  return { name: file.name ?? 'Private file', where: places.join(', ') }
}

/** Files on one page of the Storage card's largest files. */
export const STORAGE_FILES_PAGE = 10

/** The pages around `offset`: the "11–20 of 34" label and the offsets of the previous and next page (`null`: none). */
export function storageFilesPage(offset: number, shown: number, total: number): { label: string; previous: number | null; next: number | null } {
  return {
    label: `${offset + 1}–${offset + shown} of ${total}`,
    previous: offset > 0 ? Math.max(0, offset - STORAGE_FILES_PAGE) : null,
    next: offset + shown < total ? offset + STORAGE_FILES_PAGE : null,
  }
}

/** Emoji and stickers are deleted on their own settings pages, not as a stored file. */
export function canDeleteStorageFile(file: StorageFile): boolean {
  return file.uses.every((use) => use.kind !== 'emoji' && use.kind !== 'sticker')
}

function usePlace(use: StorageFileUse): string {
  switch (use.kind) {
    case 'task': return `Task “${use.title || 'Untitled'}”`
    case 'page': return `Page “${use.title || 'Untitled'}”`
    case 'channel': return `#${use.title ?? ''}`
    case 'emoji': return 'Custom emoji'
    case 'sticker': return 'Sticker'
    case 'private': return 'Private page or chat'
  }
}
