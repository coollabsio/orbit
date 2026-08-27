// Port of the chat reference ImageViewer: full-screen overlay with filename, zoom %, download, ± / reset / close.
import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { Download, Xmark } from 'reicon-react'
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

  return createPortal(
    <div className="fc-viewer" role="dialog" aria-modal="true" aria-label={attachment.fileName}>
      <div className="fc-viewer-bar">
        <div style={{ minWidth: 0 }}>
          <div className="fc-viewer-name">{attachment.fileName}</div>
          <div className="fc-viewer-zoom">{Math.round(zoom * 100)}%</div>
        </div>
        <div className="fc-viewer-actions">
          <a href={attachment.url} download={attachment.fileName} className="fc-viewer-button" title="Download image" aria-label="Download image" onClick={(e) => e.stopPropagation()}>
            <Download size={16} />
          </a>
          <button type="button" className="fc-viewer-button" title="Zoom out" aria-label="Zoom out" onClick={() => setZoom((v) => Math.max(0.5, v - 0.25))}>
            −
          </button>
          <button type="button" className="fc-viewer-button fc-viewer-button-text" title="Reset zoom" onClick={() => setZoom(1)}>
            Reset
          </button>
          <button type="button" className="fc-viewer-button" title="Zoom in" aria-label="Zoom in" onClick={() => setZoom((v) => Math.min(3, v + 0.25))}>
            +
          </button>
          <button type="button" className="fc-viewer-button" title="Close image viewer" aria-label="Close image viewer" onClick={onClose}>
            <Xmark size={16} />
          </button>
        </div>
      </div>
      <button type="button" className="fc-viewer-stage" title="Close image viewer" onClick={onClose}>
        <div className="fc-viewer-center">
          <img
            src={attachment.url}
            alt={attachment.fileName}
            draggable={false}
            style={{ transform: `scale(${zoom})`, transformOrigin: 'center' }}
            onClick={(e) => e.stopPropagation()}
          />
        </div>
      </button>
    </div>,
    document.body,
  )
}
