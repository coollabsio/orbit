import { useEffect, useState } from 'react'

const STORAGE_KEY = 'orbit:page_list_width'
export const PAGE_LIST_WIDTH = { min: 220, max: 480, default: 260 }

export function clampPageListWidth(width: number): number {
  return Math.round(Math.min(PAGE_LIST_WIDTH.max, Math.max(PAGE_LIST_WIDTH.min, width)))
}

function storedWidth(): number {
  const width = Number(window.localStorage.getItem(STORAGE_KEY))
  return width ? clampPageListWidth(width) : PAGE_LIST_WIDTH.default
}

/** The width of the desktop sidebar while it holds a page's own list (the page tree, the conversations). */
export function usePageListWidth() {
  const [width, setWidth] = useState(storedWidth)
  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, String(width))
  }, [width])
  return [width, (next: number) => setWidth(clampPageListWidth(next))] as const
}

/**
 * Drag the sidebar's right edge to resize it; arrow keys move it by 16px and a double click restores the default.
 * `onDragging` lets the sidebar drop its width transition while the pointer moves it.
 */
export function SidebarResizeHandle({ width, onResize, onDragging }: { width: number; onResize: (width: number) => void; onDragging: (dragging: boolean) => void }) {
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
      aria-valuemin={PAGE_LIST_WIDTH.min}
      aria-valuemax={PAGE_LIST_WIDTH.max}
      aria-valuenow={width}
      tabIndex={0}
      className="absolute inset-y-0 right-0 z-10 w-2 cursor-col-resize touch-none outline-none after:absolute after:inset-y-0 after:right-0 after:w-0.5 after:transition-colors hover:after:bg-primary/60 focus-visible:after:bg-primary"
      onPointerDown={startDrag}
      onDoubleClick={() => onResize(PAGE_LIST_WIDTH.default)}
      onKeyDown={(event) => {
        if (event.key === 'ArrowLeft') onResize(width - 16)
        else if (event.key === 'ArrowRight') onResize(width + 16)
        else return
        event.preventDefault()
      }}
    />
  )
}
