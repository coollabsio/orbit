import { useRef, useState } from 'react'
import { updateDocCover } from '../../../mock/actions'
import type { Doc } from '../../../mock/types'
import {
  coverObjectPosition,
  coverSlack,
  dragCoverPos,
  formatCoverPos,
  nudgeCoverPos,
  parseCoverPos,
  type CoverPos,
  type Size,
} from '../coverLib'
import { CoverSourcePanel } from './CoverSourcePanel'

/**
 * Page cover banner (the reference app's UX): hover shows Reposition / Change / Remove; repositioning
 * drags the image's focal point (arrow keys fine-tune, Enter saves, Escape cancels).
 */
export function CoverBanner({ doc }: { doc: Doc }) {
  const [repositioning, setRepositioning] = useState(false)
  const [sourceOpen, setSourceOpen] = useState(false)
  const [pos, setPos] = useState<CoverPos>(() => parseCoverPos(doc.coverPos))
  const drag = useRef<{ px: number; py: number; from: CoverPos } | null>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  const imgRef = useRef<HTMLImageElement>(null)

  const slackNow = (): Size => {
    const img = imgRef.current
    const box = boxRef.current
    if (!img || !box) return { w: 0, h: 0 }
    const rect = box.getBoundingClientRect()
    return coverSlack({ w: img.naturalWidth, h: img.naturalHeight }, { w: rect.width, h: rect.height })
  }

  const saveFraming = () => {
    setRepositioning(false)
    drag.current = null
    updateDocCover(doc.id, { coverPos: formatCoverPos(pos) })
  }

  const cancelFraming = () => {
    setRepositioning(false)
    drag.current = null
    setPos(parseCoverPos(doc.coverPos))
  }

  return (
    <div
      ref={boxRef}
      className="doc-cover"
      data-repositioning={repositioning || undefined}
      tabIndex={repositioning ? 0 : undefined}
      aria-label={repositioning ? 'Drag the image to reposition it (arrow keys to fine-tune)' : undefined}
      onPointerDown={(e) => {
        if (!repositioning) return
        if ((e.target as HTMLElement).closest('[data-cover-actions]')) return
        e.preventDefault()
        e.currentTarget.setPointerCapture(e.pointerId)
        drag.current = { px: e.clientX, py: e.clientY, from: pos }
      }}
      onPointerMove={(e) => {
        const d = drag.current
        if (!d) return
        setPos(dragCoverPos(d.from, e.clientX - d.px, e.clientY - d.py, slackNow()))
      }}
      onPointerUp={() => {
        drag.current = null
      }}
      onPointerCancel={() => {
        drag.current = null
      }}
      onKeyDown={(e) => {
        if (!repositioning) return
        const step = e.shiftKey ? 10 : 2
        const by: Record<string, [number, number]> = {
          ArrowLeft: [-step, 0],
          ArrowRight: [step, 0],
          ArrowUp: [0, -step],
          ArrowDown: [0, step],
        }
        const delta = by[e.key]
        if (delta) {
          e.preventDefault()
          setPos((p) => nudgeCoverPos(p, delta[0], delta[1]))
        } else if (e.key === 'Enter') saveFraming()
        else if (e.key === 'Escape') cancelFraming()
      }}
    >
      <img ref={imgRef} src={doc.cover ?? ''} alt="" draggable={false} style={{ objectPosition: coverObjectPosition(pos) }} />

      <div className="doc-cover-actions" data-cover-actions="" data-open={repositioning || sourceOpen || undefined}>
        {repositioning ? (
          <>
            <button type="button" className="doc-cover-btn" data-primary="true" onClick={saveFraming}>
              Save position
            </button>
            <button type="button" className="doc-cover-btn" onClick={cancelFraming}>
              Cancel
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              className="doc-cover-btn"
              onClick={() => {
                setSourceOpen(false)
                setRepositioning(true)
                boxRef.current?.focus()
              }}
            >
              Reposition
            </button>
            <button type="button" className="doc-cover-btn" onClick={() => setSourceOpen((o) => !o)}>
              Change
            </button>
            <button type="button" className="doc-cover-btn" onClick={() => updateDocCover(doc.id, { cover: null, coverPos: null })}>
              Remove
            </button>
          </>
        )}
      </div>

      {sourceOpen && !repositioning ? (
        <div className="doc-cover-source" data-cover-actions="">
          <CoverSourcePanel
            onPicked={(url) => {
              setSourceOpen(false)
              // a new image resets the framing: the old focal point means nothing on another picture
              setPos(parseCoverPos(null))
              updateDocCover(doc.id, { cover: url, coverPos: null })
            }}
          />
        </div>
      ) : null}

      {repositioning ? (
        <div className="doc-cover-hint">
          <span>Drag the image to reposition it (arrow keys to fine-tune)</span>
        </div>
      ) : null}
    </div>
  )
}
