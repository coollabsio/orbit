import type { ClipboardEvent } from 'react'

/** A file on a message, comment or page. Images can carry their size, so a list can reserve the space before they load. */
export interface Attachment {
  id: string
  fileName: string
  mimeType: string
  fileSize: number
  url: string
  width?: number
  height?: number
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`
}

export function isImage(att: Pick<Attachment, 'mimeType'>): boolean {
  return att.mimeType.startsWith('image/')
}

/** A PDF opens in the PDF viewer instead of downloading. */
export function isPdf(att: Pick<Attachment, 'mimeType'>): boolean {
  return att.mimeType === 'application/pdf'
}

/** The largest canvas side a PDF page renders at: big enough for a sharp page, small enough for phone memory. */
const PDF_MAX_CANVAS_PX = 4096

/** The pdf.js scale that draws a page `pageWidth` points wide sharply at `cssWidth` CSS pixels (2× screens at most). */
export function pdfRenderScale(cssWidth: number, pageWidth: number, devicePixelRatio: number): number {
  if (!(cssWidth > 0) || !(pageWidth > 0)) return 1
  const ratio = Math.min(Math.max(devicePixelRatio || 1, 1), 2)
  return Math.min(cssWidth * ratio, PDF_MAX_CANVAS_PX) / pageWidth
}

/** the chat reference isMedia: images and videos */
export function isMedia(att: Pick<Attachment, 'mimeType'>): boolean {
  return att.mimeType.startsWith('image/') || att.mimeType.startsWith('video/')
}

export function fileExtension(fileName: string): string {
  const ext = fileName.split('.').pop()?.slice(0, 4)
  return ext || 'FILE'
}

/** Wraps a picked / dropped / pasted file as a local attachment (object URL, no upload). */
export function fileToAttachment(file: File): Attachment {
  return {
    id: `att_${crypto.randomUUID()}`,
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
