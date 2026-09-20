import { useState } from 'react'
import { Global as Globe, Paperclip2 as Paperclip, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { appNavigate } from '@/lib/navigateBridge'
import type { DocBlock } from '@/mock/types'
import { formatSize } from '@/lib/attachmentLib'
import { ImageViewer } from '@/components/common/ImageViewer'

function domainOf(url: string): string {
  try {
    return new URL(url, window.location.origin).hostname || url
  } catch {
    return url
  }
}

const MEDIA_BASE =
  'group/media relative my-1 w-fit min-w-0 max-w-full cursor-text rounded-md px-1.5 py-0.5 first:mt-0 hover:bg-foreground/[0.02]'

/** Embed (bookmark card), image and file blocks; all removable on hover. */
export function MediaBlock({ block, onRemove }: { block: DocBlock; onRemove: () => void }) {
  const [viewerOpen, setViewerOpen] = useState(false)

  const remove = (extra?: string) => (
    <Button
      type="button"
      variant="ghost"
      size="icon-xs"
      className={cn(
        'absolute top-1.5 right-1.5 z-[2] inline-flex size-[22px] items-center justify-center rounded-full border-0 bg-black/60 text-white opacity-0 transition-[opacity,background-color] duration-150 group-hover/media:opacity-100 hover:bg-destructive hover:text-white focus-visible:opacity-100 dark:hover:bg-destructive',
        extra,
      )}
      aria-label="Remove block"
      title="Remove"
      onClick={(e) => {
        e.preventDefault()
        e.stopPropagation()
        onRemove()
      }}
    >
      <X className="size-3" />
    </Button>
  )

  if (block.type === 'image' && block.url) {
    return (
      <div className={cn(MEDIA_BASE, 'w-full')}>
        <Button
          type="button"
          variant="ghost"
          className="block h-auto w-full cursor-zoom-in overflow-hidden rounded-lg border border-border p-0 hover:bg-transparent dark:hover:bg-transparent"
          aria-label={`Open image ${block.fileName ?? ''}`}
          onClick={() => setViewerOpen(true)}
        >
          <img
            className="block h-auto w-full"
            src={block.url}
            alt={block.fileName ?? ''}
            loading={block.url.startsWith('data:') ? 'eager' : 'lazy'}
          />
        </Button>
        {remove()}
        {viewerOpen ? (
          <ImageViewer
            attachment={{
              id: block.id,
              fileName: block.fileName ?? 'image',
              mimeType: block.mimeType ?? 'image/*',
              fileSize: block.fileSize ?? 0,
              url: block.url,
            }}
            onClose={() => setViewerOpen(false)}
          />
        ) : null}
      </div>
    )
  }

  if (block.type === 'file' && block.url) {
    return (
      <div className={MEDIA_BASE}>
        <a
          href={block.url}
          download={block.fileName}
          className="flex max-w-[320px] items-center gap-2 rounded-lg border border-border bg-background py-[7px] pr-[30px] pl-2.5 text-muted-foreground no-underline shadow-[0_1px_2px_rgba(0,0,0,0.05)] transition-colors hover:bg-muted"
        >
          <Paperclip className="size-[22px]" />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="truncate text-sm font-medium text-foreground">{block.fileName}</span>
            <span className="text-xs text-muted-foreground">{formatSize(block.fileSize ?? 0)}</span>
          </span>
        </a>
        {remove('top-1/2 -translate-y-1/2')}
      </div>
    )
  }

  // embed: bookmark card; links into this app navigate in place
  const url = block.text.trim()
  const origin = window.location.origin
  const internal = url.startsWith(`${origin}/`) ? url.slice(origin.length) : url.startsWith('/') ? url : null
  return (
    <div className={MEDIA_BASE}>
      <a
        className="flex w-[480px] max-w-full items-center gap-3 rounded-lg border border-border bg-card px-3 py-2.5 no-underline transition-colors hover:bg-muted"
        href={internal ?? url}
        target={internal ? undefined : '_blank'}
        rel={internal ? undefined : 'noopener noreferrer'}
        onClick={
          internal
            ? (e) => {
                e.preventDefault()
                appNavigate(internal)
              }
            : undefined
        }
      >
        <span className="inline-flex size-[34px] shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
          <Globe className="size-[18px]" />
        </span>
        <span className="flex min-w-0 flex-col gap-0.5">
          <span className="truncate text-[13px] font-semibold text-foreground">{domainOf(url)}</span>
          <span className="truncate text-xs text-muted-foreground/70">{url}</span>
        </span>
      </a>
      {remove()}
    </div>
  )
}
