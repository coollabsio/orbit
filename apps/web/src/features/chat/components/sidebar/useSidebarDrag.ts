import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'

export type DropZone = 'before' | 'after'

/** What is dragged in the chat sidebar. */
export interface SidebarDrag {
  kind: 'channel' | 'category'
  id: string
}

/**
 * Where it would go: next to a channel row, at the end of a section (a channel over a section's header), or next to
 * a category.
 */
export type SidebarDrop =
  | { kind: 'row'; id: string; zone: DropZone }
  | { kind: 'section'; key: string }
  | { kind: 'category'; id: string; zone: DropZone }

/** A mouse drag starts after this much movement, so a click stays a click. */
const MOUSE_SLOP = 4
/** A finger that moves this far before the hold ends wants to scroll. */
const TOUCH_SLOP = 8
/** A touch drag starts after a hold of this long. */
const HOLD_MS = 350

function zoneOf(element: Element, clientY: number): DropZone {
  const rect = element.getBoundingClientRect()
  return clientY < rect.top + rect.height / 2 ? 'before' : 'after'
}

/** The drop target under the pointer. Targets say what they are with `data-drop-row`, `-section` and `-category`. */
function dropUnder(drag: SidebarDrag, clientX: number, clientY: number): SidebarDrop | null {
  const element = document.elementFromPoint(clientX, clientY)
  if (drag.kind === 'category') {
    const section = element?.closest<HTMLElement>('[data-drop-category]')
    const id = section?.dataset.dropCategory
    return section && id && id !== drag.id ? { kind: 'category', id, zone: zoneOf(section, clientY) } : null
  }
  const row = element?.closest<HTMLElement>('[data-drop-row]')
  if (row) {
    const id = row.dataset.dropRow
    return id && id !== drag.id ? { kind: 'row', id, zone: zoneOf(row, clientY) } : null
  }
  const key = element?.closest<HTMLElement>('[data-drop-section]')?.dataset.dropSection
  return key ? { kind: 'section', key } : null
}

const sameDrop = (a: SidebarDrop | null, b: SidebarDrop | null) => JSON.stringify(a) === JSON.stringify(b)

/**
 * Drag and drop in the chat sidebar with pointer events, so it works with a mouse and with a finger (HTML5 drag and
 * drop does not start from a touch on every phone). A mouse drags after a small movement; a finger after a short
 * hold, so a swipe still scrolls the list. `start` goes on `onPointerDown` of the element that is the handle.
 */
export function useSidebarDrag(onDrop: (drag: SidebarDrag, drop: SidebarDrop) => void) {
  const [drag, setDrag] = useState<SidebarDrag | null>(null)
  const [drop, setDrop] = useState<SidebarDrop | null>(null)
  // The drop runs with the handler of the newest render: the lists can change while something is dragged.
  const finish = useRef(onDrop)
  useEffect(() => {
    finish.current = onDrop
  })
  const stop = useRef<(() => void) | null>(null)
  useEffect(() => () => stop.current?.(), [])

  function start(event: ReactPointerEvent, item: SidebarDrag) {
    if (event.button !== 0 || stop.current) return
    const touch = event.pointerType !== 'mouse'
    const { clientX: startX, clientY: startY, pointerId } = event
    let active = false
    let target: SidebarDrop | null = null

    const activate = () => {
      active = true
      setDrag(item)
      document.body.style.userSelect = 'none'
    }
    const hold = touch ? setTimeout(activate, HOLD_MS) : undefined

    const end = () => {
      clearTimeout(hold)
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      window.removeEventListener('pointercancel', end)
      window.removeEventListener('touchmove', onTouchMove)
      window.removeEventListener('contextmenu', onContextMenu, true)
      document.body.style.userSelect = ''
      stop.current = null
      setDrag(null)
      setDrop(null)
    }
    const onMove = (move: PointerEvent) => {
      if (move.pointerId !== pointerId) return
      if (!active) {
        const distance = Math.hypot(move.clientX - startX, move.clientY - startY)
        if (touch ? distance > TOUCH_SLOP : distance <= MOUSE_SLOP) {
          if (touch) end()
          return
        }
        if (touch) return
        activate()
      }
      const next = dropUnder(item, move.clientX, move.clientY)
      if (sameDrop(next, target)) return
      target = next
      setDrop(next)
    }
    const onUp = (up: PointerEvent) => {
      if (up.pointerId !== pointerId) return
      const dropped = active ? target : null
      if (active) {
        // The click that follows a drag must not open the row under the pointer.
        const swallow = (click: MouseEvent) => {
          click.preventDefault()
          click.stopPropagation()
        }
        window.addEventListener('click', swallow, { capture: true, once: true })
        setTimeout(() => window.removeEventListener('click', swallow, true), 0)
      }
      end()
      if (dropped) finish.current(item, dropped)
    }
    // While a finger drags, the list must not scroll.
    const onTouchMove = (move: TouchEvent) => {
      if (active && move.cancelable) move.preventDefault()
    }
    // A hold is the start of a drag here, not the request for the context menu.
    const onContextMenu = (menu: MouseEvent) => {
      if (!touch) return
      menu.preventDefault()
      menu.stopPropagation()
    }

    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
    window.addEventListener('pointercancel', end)
    window.addEventListener('touchmove', onTouchMove, { passive: false })
    window.addEventListener('contextmenu', onContextMenu, true)
    stop.current = end
  }

  return { drag, drop, start }
}
