import { useRef, useState } from 'react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import {
  coverObjectPosition,
  coverSlack,
  dragCoverPos,
  formatCoverPos,
  nudgeCoverPos,
  parseCoverPos,
  type CoverPos,
  type Size,
} from '@/features/docs/coverLib'
import { CoverSourcePanel } from './CoverSourcePanel'

/**
 * Page cover banner (the reference app's UX): hover shows Reposition / Change / Remove; repositioning
 * drags the image's focal point (arrow keys fine-tune, Enter saves, Escape cancels).
 */
export function CoverBanner({
  url,
  position,
  onChange,
  onUpload,
  readOnly = false,
}: {
  url: string
  /** No reposition / change / remove controls (locked page). */
  readOnly?: boolean
  position: string | null
  /** Uploads a new cover image to the page; resolves to its URL. */
  onUpload?: (file: File) => Promise<string>
  /** A new cover (or null to remove it) and/or a new focal point. */
  onChange: (patch: { cover_url?: string | null; cover_position?: string | null }) => void
}) {
  const [repositioning, setRepositioning] = useState(false)
  const [sourceOpen, setSourceOpen] = useState(false)
  const [pos, setPos] = useState<CoverPos>(() => parseCoverPos(position))
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
    onChange({ cover_position: formatCoverPos(pos) })
  }

  const cancelFraming = () => {
    setRepositioning(false)
    drag.current = null
    setPos(parseCoverPos(position))
  }

  return (
    <div
      ref={boxRef}
      className="group/cover relative mx-[-24px] mt-[-32px] mb-6 h-[224px] shrink-0 overflow-hidden data-[repositioning]:cursor-grab data-[repositioning]:touch-none data-[repositioning]:select-none data-[repositioning]:outline-none data-[repositioning]:active:cursor-grabbing"
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
      <img
        ref={imgRef}
        className="pointer-events-none size-full object-cover"
        src={url}
        alt=""
        draggable={false}
        style={{ objectPosition: coverObjectPosition(pos) }}
      />

      {readOnly ? null : <div
        className="absolute top-2 right-2 z-[5] hidden gap-1 group-hover/cover:flex data-[open]:flex"
        data-cover-actions=""
        data-open={repositioning || sourceOpen || undefined}
      >
        {repositioning ? (
          <>
            <Button type="button" className={cn(COVER_BTN, COVER_BTN_PRIMARY)} onClick={saveFraming}>
              Save position
            </Button>
            <Button type="button" variant="outline" className={cn(COVER_BTN, COVER_BTN_OUTLINE)} onClick={cancelFraming}>
              Cancel
            </Button>
          </>
        ) : (
          <>
            <Button
              type="button"
              variant="outline"
              className={cn(COVER_BTN, COVER_BTN_OUTLINE)}
              onClick={() => {
                setSourceOpen(false)
                setRepositioning(true)
                boxRef.current?.focus()
              }}
            >
              Reposition
            </Button>
            <Button
              type="button"
              variant="outline"
              className={cn(COVER_BTN, COVER_BTN_OUTLINE)}
              onClick={() => setSourceOpen((o) => !o)}
            >
              Change
            </Button>
            <Button
              type="button"
              variant="outline"
              className={cn(COVER_BTN, COVER_BTN_OUTLINE)}
              onClick={() => onChange({ cover_url: null, cover_position: null })}
            >
              Remove
            </Button>
          </>
        )}
      </div>}

      {sourceOpen && !repositioning ? (
        <div
          className="absolute inset-x-2 top-[44px] z-[6] mx-auto max-w-[420px] rounded-lg border border-border bg-background/95 p-3 shadow-md backdrop-blur-[6px]"
          data-cover-actions=""
        >
          <CoverSourcePanel
            onUpload={onUpload}
            onPicked={(next) => {
              setSourceOpen(false)
              // a new image resets the framing: the old focal point means nothing on another picture
              setPos(parseCoverPos(null))
              onChange({ cover_url: next, cover_position: null })
            }}
          />
        </div>
      ) : null}

      {repositioning ? (
        <div className="pointer-events-none absolute inset-x-0 bottom-2 flex justify-center">
          <span className="rounded-full bg-background/85 px-3 py-1 text-xs text-foreground shadow-md backdrop-blur-[4px]">
            Drag the image to reposition it (arrow keys to fine-tune)
          </span>
        </div>
      ) : null}
    </div>
  )
}

const COVER_BTN = 'h-auto min-h-[26px] rounded-md px-2.5 py-[3px] text-xs font-medium backdrop-blur-[4px]'
const COVER_BTN_OUTLINE =
  'border-border bg-background/80 text-foreground hover:bg-background dark:border-border dark:bg-background/80 dark:hover:bg-background'
const COVER_BTN_PRIMARY = 'border-primary bg-primary text-primary-foreground hover:bg-primary'
