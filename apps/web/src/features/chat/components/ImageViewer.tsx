// Port of the chat reference ImageViewer: full-screen overlay with filename, zoom %, download, ± / reset / close.
import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { Download, X } from 'lucide-react'
import type { Attachment } from '../../../mock/types'

export function ImageViewer({ attachment, onClose }: { attachment: Attachment; onClose: () => void }) {
  const [zoom, setZoom] = useState(1)

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') onClose()
      if (event.key === '+' || event.key === '=') setZoom((value) => Math.min(3, value + 0.25))
      if (event.key === '-' || event.key === '_') setZoom((value) => Math.max(0.5, value - 0.25))
      if (event.key === '0') setZoom(1)
    }
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    window.addEventListener('keydown', handleKeyDown)
    return () => {
      document.body.style.overflow = previousOverflow
      window.removeEventListener('keydown', handleKeyDown)
    }
  }, [onClose])

  const barButton = 'grid size-9 place-items-center rounded-md bg-white/10 text-white transition-colors hover:bg-white/15'

  return createPortal(
    <div
      className="fixed inset-0 z-[100] flex flex-col bg-black/90 text-white backdrop-blur-sm duration-150 animate-in fade-in motion-reduce:animate-none"
      role="dialog"
      aria-modal="true"
      aria-label={attachment.fileName}
    >
      <div className="flex h-14 shrink-0 items-center justify-between border-b border-white/10 px-4 duration-200 animate-in slide-in-from-top-2 motion-reduce:animate-none">
        <div className="min-w-0">
          <div className="truncate text-sm font-bold">{attachment.fileName}</div>
          <div className="text-xs font-semibold text-white/55">{Math.round(zoom * 100)}%</div>
        </div>
        <div className="flex items-center gap-2">
          <a href={attachment.url} download={attachment.fileName} className={barButton} title="Download image" aria-label="Download image" onClick={(e) => e.stopPropagation()}>
            <Download className="size-4" />
          </a>
          <button type="button" className={`${barButton} text-lg font-bold`} title="Zoom out" aria-label="Zoom out" onClick={() => setZoom((v) => Math.max(0.5, v - 0.25))}>
            −
          </button>
          <button type="button" className="flex h-9 items-center rounded-md bg-white/10 px-3 text-xs font-bold text-white transition-colors hover:bg-white/15" title="Reset zoom" onClick={() => setZoom(1)}>
            Reset
          </button>
          <button type="button" className={`${barButton} text-lg font-bold`} title="Zoom in" aria-label="Zoom in" onClick={() => setZoom((v) => Math.min(3, v + 0.25))}>
            +
          </button>
          <button type="button" className={barButton} title="Close image viewer" aria-label="Close image viewer" onClick={onClose}>
            <X className="size-4" />
          </button>
        </div>
      </div>
      <button type="button" className="min-h-0 flex-1 cursor-zoom-out overflow-auto p-6" title="Close image viewer" onClick={onClose}>
        <div className="flex min-h-full items-center justify-center">
          <img
            src={attachment.url}
            alt={attachment.fileName}
            draggable={false}
            className="max-h-[calc(100vh-7rem)] max-w-[calc(100vw-3rem)] rounded-md object-contain shadow-[0_25px_50px_-12px_rgba(0,0,0,0.6)] transition-transform select-none duration-200 animate-in fade-in zoom-in-95 motion-reduce:animate-none"
            style={{ transform: `scale(${zoom})`, transformOrigin: 'center' }}
            onClick={(e) => e.stopPropagation()}
          />
        </div>
      </button>
    </div>,
    document.body,
  )
}
