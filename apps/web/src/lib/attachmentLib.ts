import type { ClipboardEvent } from 'react'
import { nextId } from '@/mock/store'
import type { Attachment } from '@/mock/types'

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

/** Wraps a picked / dropped / pasted file as a mock attachment (object URL, no upload). */
export function fileToAttachment(file: File): Attachment {
  return {
    id: nextId('att'),
    fileName: file.name || `pasted-${Date.now()}.${(file.type.split('/')[1] || 'bin').replace(/[^a-z0-9]/gi, '')}`,
    mimeType: file.type || 'application/octet-stream',
    fileSize: file.size,
    url: URL.createObjectURL(file),
  }
}

/** Files carried by a paste event (screenshots, copied images, files from the file manager), de-duplicated. */
export function clipboardFiles(event: ClipboardEvent<HTMLElement>): File[] {
  const data = event.clipboardData
  if (!data) return []
  const found = [
    ...Array.from(data.files),
    ...Array.from(data.items)
      .filter((item) => item.kind === 'file')
      .map((item) => item.getAsFile())
      .filter((file): file is File => Boolean(file)),
  ]
  const seen = new Set<string>()
  return found.filter((file) => {
    const key = `${file.name}:${file.type}:${file.size}`
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}
