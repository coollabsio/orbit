// Port of the chat reference Attachments: image grid (1 / 2 / 3-hero / 4+N) that opens the ImageViewer,
// plus file cards for non-images (paperclip, name, size, download on hover). Self-contained Tailwind styling.
import { useState } from 'react'
import { Download, Paperclip, X } from 'lucide-react'
import type { Attachment } from '../../../mock/types'
import { formatSize, isImage } from '../attachmentLib'
import { ImageViewer } from './ImageViewer'

const removeButton =
  'absolute top-1.5 right-1.5 z-[2] inline-flex size-[22px] items-center justify-center rounded-full bg-black/60 text-white opacity-0 transition-[opacity,background-color] hover:bg-destructive focus-visible:opacity-100 group-hover:opacity-100'

export function Attachments({
  attachments,
  hasTextContent = true,
  onRemove,
}: {
  attachments: Attachment[]
  hasTextContent?: boolean
  /** When given, every attachment gets a remove button. */
  onRemove?: (attachmentId: string) => void
}) {
  const [viewerImage, setViewerImage] = useState<Attachment | null>(null)
  if (attachments.length === 0) return null

  const images = attachments.filter(isImage)
  const otherFiles = attachments.filter((att) => !isImage(att))
  const single = images.length === 1

  return (
    <div className={`flex flex-col gap-2 ${hasTextContent ? 'mt-2' : 'mt-1'}`} data-no-text={hasTextContent ? undefined : 'true'}>
      {images.length > 0 ? (
        <div
          className={`grid max-w-[512px] gap-1 ${images.length === 3 ? 'grid-cols-3' : 'grid-cols-2'}`}
          data-count={Math.min(images.length, 4)}
        >
          {images.slice(0, 4).map((att, index) => {
            const hero = images.length === 3 && index === 0
            return (
              <div
                key={att.id}
                className={`group relative overflow-hidden rounded-md border border-border transition-opacity hover:opacity-90 ${
                  single ? 'col-span-2 aspect-auto w-fit' : 'aspect-square'
                } ${hero ? 'col-span-2 row-span-2' : ''}`}
                data-single={single || undefined}
                data-hero={hero ? 'true' : undefined}
              >
                <button type="button" className="block size-full cursor-pointer" aria-label={`Open image ${att.fileName}`} onClick={() => setViewerImage(att)}>
                  <img
                    src={att.url}
                    alt={att.fileName}
                    loading={att.url.startsWith('data:') ? 'eager' : 'lazy'}
                    className={single ? 'block h-auto max-h-[300px] w-auto max-w-full object-contain' : 'block size-full object-cover'}
                  />
                </button>
                {images.length > 4 && index === 3 ? (
                  <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-black/60 text-lg font-semibold text-white">+{images.length - 4}</div>
                ) : null}
                {onRemove ? (
                  <button type="button" className={removeButton} aria-label={`Remove ${att.fileName}`} title="Remove" onClick={() => onRemove(att.id)}>
                    <X className="size-3" />
                  </button>
                ) : null}
              </div>
            )
          })}
        </div>
      ) : null}

      {otherFiles.length > 0 ? (
        <div className="flex max-w-md flex-col gap-2">
          {otherFiles.map((att) => (
            <span key={att.id} className="group relative block">
              <a href={att.url} download={att.fileName} className="group/card flex items-center gap-3 rounded-lg border border-border bg-background p-3 text-muted-foreground shadow-sm transition-colors hover:bg-muted">
                <Paperclip className="size-10 shrink-0" />
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-sm font-medium text-foreground">{att.fileName}</span>
                  <span className="text-xs text-muted-foreground">{formatSize(att.fileSize)}</span>
                </span>
                <span className="opacity-0 transition-opacity group-hover/card:opacity-100">
                  <Download className="size-5" />
                </span>
              </a>
              {onRemove ? (
                <button type="button" className={removeButton} aria-label={`Remove ${att.fileName}`} title="Remove" onClick={() => onRemove(att.id)}>
                  <X className="size-3" />
                </button>
              ) : null}
            </span>
          ))}
        </div>
      ) : null}

      {viewerImage ? <ImageViewer attachment={viewerImage} onClose={() => setViewerImage(null)} /> : null}
    </div>
  )
}
