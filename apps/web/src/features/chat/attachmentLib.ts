import type { Attachment } from '../../mock/types'

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

export function isImage(att: Pick<Attachment, 'mimeType'>): boolean {
  return att.mimeType.startsWith('image/')
}

/** the chat reference isMedia: images and videos */
export function isMedia(att: Pick<Attachment, 'mimeType'>): boolean {
  return att.mimeType.startsWith('image/') || att.mimeType.startsWith('video/')
}

export function fileExtension(fileName: string): string {
  const ext = fileName.split('.').pop()?.slice(0, 4)
  return ext || 'FILE'
}
