// Port of the chat reference ImageViewer: full-screen overlay with filename, zoom %, download, ± / reset / close.
import { useState, type KeyboardEvent } from 'react'
import { Download, Xmark as X } from 'reicon-react'
import type { Attachment } from '@/mock/types'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent } from '@/components/ui/dialog'

export function ImageViewer({ attachment, onClose }: { attachment: Attachment; onClose: () => void }) {
  const [zoom, setZoom] = useState(1)

  // Escape, focus trap and scroll lock come from the Dialog; focus stays inside it, so zoom keys bubble here.
  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === '+' || event.key === '=') setZoom((value) => Math.min(3, value + 0.25))
    if (event.key === '-' || event.key === '_') setZoom((value) => Math.max(0.5, value - 0.25))
    if (event.key === '0') setZoom(1)
  }

  const barButton = 'grid size-9 place-items-center rounded-md bg-white/10 text-white transition-colors hover:bg-white/15'
  const barButtonClass = 'size-9 rounded-md border-0 bg-white/10 text-white transition-colors hover:bg-white/15 hover:text-white dark:hover:bg-white/15'

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      <DialogContent
        showCloseButton={false}
        aria-label={attachment.fileName}
        onKeyDown={handleKeyDown}
        className="top-0 right-0 left-0 z-[100] flex h-[var(--app-height,100svh)] max-h-[var(--app-height,100svh)] w-full max-w-none translate-x-0 translate-y-0 flex-col gap-0 overflow-hidden rounded-none bg-black/90 p-0 pt-[env(safe-area-inset-top,0px)] pr-[env(safe-area-inset-right,0px)] pb-[env(safe-area-inset-bottom,0px)] pl-[env(safe-area-inset-left,0px)] text-white ring-0 backdrop-blur-sm duration-150 motion-reduce:animate-none sm:max-w-none"
      >
        <div className="flex h-14 shrink-0 items-center justify-between gap-3 overflow-x-auto border-b border-white/10 px-4 duration-200 animate-in slide-in-from-top-2 motion-reduce:animate-none">
          <div className="min-w-0">
            <div className="truncate text-sm font-bold">{attachment.fileName}</div>
            <div className="text-xs font-semibold text-white/55">{Math.round(zoom * 100)}%</div>
          </div>
          <div className="flex items-center gap-2">
            <a href={attachment.url} download={attachment.fileName} className={barButton} title="Download image" aria-label="Download image" onClick={(e) => e.stopPropagation()}>
              <Download className="size-4" />
            </a>
            <Button type="button" variant="ghost" size="icon-lg" className={`${barButtonClass} text-lg font-bold`} title="Zoom out" aria-label="Zoom out" onClick={() => setZoom((v) => Math.max(0.5, v - 0.25))}>
              −
            </Button>
            <Button type="button" variant="ghost" size="lg" className="h-9 rounded-md border-0 bg-white/10 px-3 text-xs font-bold text-white transition-colors hover:bg-white/15 hover:text-white dark:hover:bg-white/15" title="Reset zoom" onClick={() => setZoom(1)}>
              Reset
            </Button>
            <Button type="button" variant="ghost" size="icon-lg" className={`${barButtonClass} text-lg font-bold`} title="Zoom in" aria-label="Zoom in" onClick={() => setZoom((v) => Math.min(3, v + 0.25))}>
              +
            </Button>
            <Button type="button" variant="ghost" size="icon-lg" className={barButtonClass} title="Close image viewer" aria-label="Close image viewer" onClick={onClose}>
              <X className="size-4" />
            </Button>
          </div>
        </div>
        <Button type="button" variant="ghost" className="flex h-auto min-h-0 w-full flex-1 shrink! cursor-zoom-out items-center justify-center overflow-auto rounded-none border-0 p-2 font-normal whitespace-normal select-auto hover:bg-transparent sm:p-6 dark:hover:bg-transparent active:not-aria-[haspopup]:translate-y-0" title="Close image viewer" onClick={onClose}>
          <img
            src={attachment.url}
            alt={attachment.fileName}
            draggable={false}
            className="max-h-full max-w-full origin-center rounded-md object-contain shadow-[0_25px_50px_-12px_rgba(0,0,0,0.6)] transition-transform select-none duration-200 animate-in fade-in zoom-in-95 motion-reduce:animate-none"
            style={{ transform: `scale(${zoom})` }}
            onClick={(e) => e.stopPropagation()}
          />
        </Button>
      </DialogContent>
    </Dialog>
  )
}
