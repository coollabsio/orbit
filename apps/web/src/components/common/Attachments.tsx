// Port of the chat reference Attachments: image grid (1 / 2 / 3-hero / 4+N) that opens the ImageViewer,
// plus file cards for non-images (paperclip, name, size, download on hover). Self-contained Tailwind styling.
import { useState } from 'react'
import { Download, Paperclip2 as Paperclip, Xmark as X } from 'reicon-react'
import { formatSize, isImage, type Attachment } from '@/lib/attachmentLib'
import { ImageViewer } from './ImageViewer'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { cn } from 'cn'

const SINGLE_IMAGE_MAX_HEIGHT = 300

/** Dark round × over an attachment. With a mouse it shows on hover or keyboard focus; on touch it is always visible.
    `after:` grows the 22px circle to a 30px hit area. */
function RemoveAttachmentButton({ attachment, onRemove }: { attachment: Attachment; onRemove: (attachmentId: string) => void }) {
  return (
    <Tip label="Remove">
      <Button
        data-slot="attachment-remove"
        variant="ghost"
        size="icon-xs"
        className="absolute top-1.5 right-1.5 z-[2] size-[22px] rounded-full bg-black/60 text-white transition-[opacity,background-color] duration-150 after:absolute after:-inset-1 hover:bg-destructive hover:text-white focus-visible:opacity-100 hover-fine:opacity-0 hover-fine:group-hover:opacity-100 dark:hover:bg-destructive"
        aria-label={`Remove ${attachment.fileName}`}
        onClick={() => onRemove(attachment.id)}
      >
        <X className="size-3" />
      </Button>
    </Tip>
  )
}

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
    <div className={cn('flex flex-col gap-2', hasTextContent ? 'mt-2' : 'mt-1')} data-no-text={hasTextContent ? undefined : 'true'}>
      {images.length > 0 ? (
        <div
          className={cn('grid max-w-[512px] gap-1', images.length === 3 ? 'grid-cols-3' : 'grid-cols-2')}
          data-count={Math.min(images.length, 4)}
        >
          {images.slice(0, 4).map((att, index) => {
            const hero = images.length === 3 && index === 0
            return (
              <div
                key={att.id}
                className={cn(
                  'group relative overflow-hidden rounded-md border border-border transition-opacity hover:opacity-90',
                  single ? 'col-span-2 aspect-auto w-fit' : 'aspect-square',
                  hero && 'col-span-2 row-span-2',
                )}
                data-single={single || undefined}
                data-hero={hero ? 'true' : undefined}
              >
                <Button type="button" variant="ghost" className="block size-full cursor-pointer rounded-none border-0 p-0 hover:bg-transparent dark:hover:bg-transparent active:not-aria-[haspopup]:translate-y-0" aria-label={`Open image ${att.fileName}`} onClick={() => setViewerImage(att)}>
                  <img
                    src={att.url}
                    alt={att.fileName}
                    loading={att.url.startsWith('data:') ? 'eager' : 'lazy'}
                    width={att.width}
                    height={att.height}
                    // a single image with a known size holds its place before it loads (at most 300px high)
                    style={single && att.width && att.height
                      ? { aspectRatio: `${att.width} / ${att.height}`, width: Math.min(att.width, Math.round((SINGLE_IMAGE_MAX_HEIGHT * att.width) / att.height)) }
                      : undefined}
                    className={single ? 'block h-auto max-h-[300px] w-auto max-w-full object-contain' : 'block size-full object-cover'}
                  />
                </Button>
                {images.length > 4 && index === 3 ? (
                  <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-black/60 text-lg font-semibold text-white">+{images.length - 4}</div>
                ) : null}
                {onRemove ? (
                  <RemoveAttachmentButton attachment={att} onRemove={onRemove} />
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
                <span className="transition-opacity duration-150 group-focus-visible/card:opacity-100 hover-fine:opacity-0 hover-fine:group-hover/card:opacity-100">
                  <Download className="size-5" />
                </span>
              </a>
              {onRemove ? (
                <RemoveAttachmentButton attachment={att} onRemove={onRemove} />
              ) : null}
            </span>
          ))}
        </div>
      ) : null}

      {viewerImage ? <ImageViewer attachment={viewerImage} onClose={() => setViewerImage(null)} /> : null}
    </div>
  )
}
