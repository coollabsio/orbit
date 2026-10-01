import { useRef, useState } from 'react'
import type { WidthRange } from './storedWidth'

const KEY_STEP = 16

/**
 * The drag handle on the edge of a resizable pane (its parent must be `relative`). `edge` is the side of the pane the
 * handle sits on: dragging away from the pane makes it wider. Arrow keys resize too. Hidden on phones.
 */
export function ResizeHandle({
  edge,
  width,
  range,
  label,
  onResize,
}: {
  edge: 'left' | 'right'
  width: number
  range: WidthRange
  label: string
  onResize: (width: number) => void
}) {
  const drag = useRef<{ x: number; width: number } | null>(null)
  const [dragging, setDragging] = useState(false)
  const direction = edge === 'right' ? 1 : -1
  const stop = () => {
    drag.current = null
    setDragging(false)
  }
  return (
    <div
      data-slot="resize-handle"
      data-edge={edge}
      data-dragging={dragging || undefined}
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={width}
      aria-valuemin={range.min}
      aria-valuemax={range.max}
      tabIndex={0}
      className="absolute inset-y-0 z-10 w-2 cursor-col-resize touch-none outline-none select-none after:absolute after:inset-y-0 after:left-[3px] after:w-0.5 after:bg-transparent focus-visible:after:bg-ring data-dragging:after:bg-ring data-[edge=left]:-left-1 data-[edge=right]:-right-1 hover-fine:hover:after:bg-ring max-[899px]:hidden"
      onPointerDown={(event) => {
        if (event.button !== 0) return
        event.preventDefault()
        event.currentTarget.setPointerCapture(event.pointerId)
        drag.current = { x: event.clientX, width }
        setDragging(true)
      }}
      onPointerMove={(event) => {
        if (drag.current) onResize(drag.current.width + (event.clientX - drag.current.x) * direction)
      }}
      onPointerUp={stop}
      onPointerCancel={stop}
      onDoubleClick={() => onResize(range.initial)}
      onKeyDown={(event) => {
        const step = event.key === 'ArrowRight' ? KEY_STEP : event.key === 'ArrowLeft' ? -KEY_STEP : 0
        if (step === 0) return
        event.preventDefault()
        onResize(width + step * direction)
      }}
    />
  )
}
