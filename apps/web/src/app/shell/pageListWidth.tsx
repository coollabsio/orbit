import { useEffect, useState } from 'react'

export interface SidebarWidthRange {
  storageKey: string
  min: number
  max: number
  default: number
}

/** The desktop sidebar while it holds a page's own list (the page tree, the conversations). */
export const PAGE_LIST_WIDTH: SidebarWidthRange = { storageKey: 'orbit:page_list_width', min: 220, max: 480, default: 260 }
/** The desktop sidebar on the other pages (Tasks, Home, Activity, …). */
export const NAV_WIDTH: SidebarWidthRange = { storageKey: 'orbit:sidebar_width', min: 200, max: 400, default: 224 }

function clamp(range: SidebarWidthRange, width: number): number {
  return Math.round(Math.min(range.max, Math.max(range.min, width)))
}

/** The saved width of the sidebar in `range`, and a setter that keeps it in range. */
export function useSidebarWidth(range: SidebarWidthRange) {
  const [width, setWidth] = useState(() => {
    const stored = Number(window.localStorage.getItem(range.storageKey))
    return stored ? clamp(range, stored) : range.default
  })
  useEffect(() => {
    window.localStorage.setItem(range.storageKey, String(width))
  }, [range.storageKey, width])
  return [width, (next: number) => setWidth(clamp(range, next))] as const
}

/**
 * Drag the sidebar's right edge to resize it; arrow keys move it by 16px and a double click restores the default.
 * `onDragging` lets the sidebar drop its width transition while the pointer moves it.
 */
export function SidebarResizeHandle({ width, range, onResize, onDragging }: { width: number; range: SidebarWidthRange; onResize: (width: number) => void; onDragging: (dragging: boolean) => void }) {
  function startDrag(event: React.PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return
    event.preventDefault()
    const handle = event.currentTarget
    handle.setPointerCapture(event.pointerId)
    const startX = event.clientX
    const startWidth = width
    onDragging(true)
    const move = (moveEvent: PointerEvent) => onResize(startWidth + moveEvent.clientX - startX)
    const end = () => {
      onDragging(false)
      handle.removeEventListener('pointermove', move)
      handle.removeEventListener('pointerup', end)
      handle.removeEventListener('pointercancel', end)
    }
    handle.addEventListener('pointermove', move)
    handle.addEventListener('pointerup', end)
    handle.addEventListener('pointercancel', end)
  }

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize sidebar"
      aria-valuemin={range.min}
      aria-valuemax={range.max}
      aria-valuenow={width}
      tabIndex={0}
      className="absolute inset-y-0 right-0 z-10 w-2 cursor-col-resize touch-none outline-none after:absolute after:inset-y-0 after:right-0 after:w-0.5 after:transition-colors hover:after:bg-primary/60 focus-visible:after:bg-primary"
      onPointerDown={startDrag}
      onDoubleClick={() => onResize(range.default)}
      onKeyDown={(event) => {
        if (event.key === 'ArrowLeft') onResize(width - 16)
        else if (event.key === 'ArrowRight') onResize(width + 16)
        else return
        event.preventDefault()
      }}
    />
  )
}
