// Port of the chat reference ImageViewer: full-screen overlay with filename, zoom %, download, ± / reset / close.
import { useRef, useState, type ComponentProps, type KeyboardEvent, type TouchEvent } from 'react'
import { Download, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import type { Attachment } from '@/mock/types'
import { Button, buttonVariants } from '@/components/ui/button'
import { Dialog, DialogContent } from '@/components/ui/dialog'

const MIN_ZOOM = 0.5
const MAX_ZOOM = 4

type Point = { x: number; y: number }
/** The touch gesture in progress: where it started, with the zoom and pan it started from. */
type Gesture = { distance: number; center: Point; zoom: number; pan: Point }

const clampZoom = (value: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, value))

function touchCenter(touches: TouchEvent['touches']): Point {
  const [a, b = a] = [touches[0], touches[1]]
  return { x: (a.clientX + b.clientX) / 2, y: (a.clientY + b.clientY) / 2 }
}

function touchDistance(touches: TouchEvent['touches']): number {
  return touches.length < 2 ? 0 : Math.hypot(touches[0].clientX - touches[1].clientX, touches[0].clientY - touches[1].clientY)
}

/** A light-on-dark control in the viewer's top bar. */
function ViewerBarButton({ className, size = 'icon-lg', ...props }: ComponentProps<typeof Button>) {
  return (
    <Button
      data-slot="image-viewer-button"
      variant="ghost"
      size={size}
      className={cn('bg-white/10 text-white hover:bg-white/15 hover:text-white dark:hover:bg-white/15', className)}
      {...props}
    />
  )
}

export function ImageViewer({ attachment, onClose }: { attachment: Attachment; onClose: () => void }) {
  const [zoom, setZoomState] = useState(1)
  const [pan, setPan] = useState<Point>({ x: 0, y: 0 })
  const [gesturing, setGesturing] = useState(false)
  const gesture = useRef<Gesture | null>(null)
  // Closing plays the dialog's exit first; the caller unmounts the viewer once it has finished.
  const [open, setOpen] = useState(true)
  const close = () => setOpen(false)

  // Back at 100% or less there is nothing to pan to, so the image goes back to the center.
  function setZoom(value: number, nextPan: Point = pan) {
    const next = clampZoom(value)
    setZoomState(next)
    setPan(next <= 1 ? { x: 0, y: 0 } : nextPan)
  }

  // Two fingers pinch to zoom (and move the image with them); one finger pans a zoomed image.
  function startGesture(event: TouchEvent) {
    gesture.current = { distance: touchDistance(event.touches), center: touchCenter(event.touches), zoom, pan }
    setGesturing(true)
  }

  function moveGesture(event: TouchEvent) {
    const start = gesture.current
    if (!start) return
    const center = touchCenter(event.touches)
    const distance = touchDistance(event.touches)
    const pinching = event.touches.length >= 2 && start.distance > 0
    if (!pinching && start.zoom <= 1) return
    setZoom(pinching ? (start.zoom * distance) / start.distance : start.zoom, {
      x: start.pan.x + center.x - start.center.x,
      y: start.pan.y + center.y - start.center.y,
    })
  }

  function endGesture(event: TouchEvent) {
    // Lifting one of two fingers goes on as a pan from where the image is now.
    if (event.touches.length > 0) startGesture(event)
    else {
      gesture.current = null
      setGesturing(false)
    }
  }

  // Escape, focus trap and scroll lock come from the Dialog; focus stays inside it, so zoom keys bubble here.
  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === '+' || event.key === '=') setZoom(zoom + 0.25)
    if (event.key === '-' || event.key === '_') setZoom(zoom - 0.25)
    if (event.key === '0') setZoom(1)
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) setOpen(false)
      }}
      onOpenChangeComplete={(next) => {
        if (!next) onClose()
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
            <div className="text-xs font-semibold text-white/55 tabular-nums">{Math.round(zoom * 100)}%</div>
          </div>
          <div className="flex items-center gap-2">
            <a
              data-slot="image-viewer-button"
              href={attachment.url}
              download={attachment.fileName}
              className={cn(buttonVariants({ variant: 'ghost', size: 'icon-lg' }), 'bg-white/10 text-white hover:bg-white/15 hover:text-white dark:hover:bg-white/15')}
              title="Download image"
              aria-label="Download image"
              onClick={(e) => e.stopPropagation()}
            >
              <Download className="size-4" />
            </a>
            <ViewerBarButton className="text-lg font-bold" title="Zoom out" aria-label="Zoom out" onClick={() => setZoom(zoom - 0.25)}>
              −
            </ViewerBarButton>
            <ViewerBarButton size="lg" className="px-3 text-xs font-bold" title="Reset zoom" onClick={() => setZoom(1)}>
              Reset
            </ViewerBarButton>
            <ViewerBarButton className="text-lg font-bold" title="Zoom in" aria-label="Zoom in" onClick={() => setZoom(zoom + 0.25)}>
              +
            </ViewerBarButton>
            <ViewerBarButton title="Close image viewer" aria-label="Close image viewer" onClick={close}>
              <X className="size-4" />
            </ViewerBarButton>
          </div>
        </div>
        <Button type="button" variant="ghost" className="flex h-auto min-h-0 w-full flex-1 shrink! cursor-zoom-out items-center justify-center overflow-auto rounded-none border-0 p-2 font-normal whitespace-normal select-auto hover:bg-transparent touch-none sm:p-6 dark:hover:bg-transparent active:not-aria-[haspopup]:translate-y-0"
          title="Close image viewer"
          onClick={close}
          onTouchStart={startGesture}
          onTouchMove={moveGesture}
          onTouchEnd={endGesture}
          onTouchCancel={endGesture}
        >
          <img
            src={attachment.url}
            alt={attachment.fileName}
            draggable={false}
            className={cn(
              'max-h-full max-w-full origin-center rounded-md object-contain shadow-[0_25px_50px_-12px_rgba(0,0,0,0.6)] select-none duration-200 animate-in fade-in zoom-in-95 motion-reduce:animate-none',
              // No transition while a finger moves the image: it must follow the finger without lag.
              gesturing ? 'transition-none' : 'transition-transform',
            )}
            style={{ transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})` }}
            onClick={(e) => e.stopPropagation()}
          />
        </Button>
      </DialogContent>
    </Dialog>
  )
}
