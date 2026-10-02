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

/**
 * The copy of the dragged row that follows the pointer, as in Discord: a little transparent, with a shadow. It goes
 * into the sidebar, so it has the colours of the chat theme, and it takes no pointer events, so the row under it is
 * still found.
 */
function makeGhost(source: HTMLElement): { element: HTMLElement; rect: DOMRect } {
  const rect = source.getBoundingClientRect()
  const element = source.cloneNode(true) as HTMLElement
  element.removeAttribute('data-drop-row')
  element.removeAttribute('data-drop')
  element.setAttribute('data-slot', 'chat-drag-ghost')
  element.setAttribute('aria-hidden', 'true')
  Object.assign(element.style, {
    position: 'fixed',
    top: '0',
    left: '0',
    width: `${rect.width}px`,
    height: `${rect.height}px`,
    margin: '0',
    zIndex: '60',
    pointerEvents: 'none',
    opacity: '0.75',
    borderRadius: '6px',
    background: 'var(--popover)',
    boxShadow: '0 8px 24px rgb(0 0 0 / 0.3)',
    cursor: 'grabbing',
    willChange: 'transform',
  })
  ;(source.closest('[data-slot="chat-sidebar"]') ?? document.body).appendChild(element)
  return { element, rect }
}

/** Closer to the top or bottom of the list than this, a drag scrolls the list. */
const SCROLL_EDGE = 36

const sameDrop = (a: SidebarDrop | null, b: SidebarDrop | null) => JSON.stringify(a) === JSON.stringify(b)

/**
 * Drag and drop in the chat sidebar with pointer events, so it works with a mouse and with a finger (HTML5 drag and
 * drop does not start from a touch on every phone). A mouse drags after a small movement; a finger after a short
 * hold, so a swipe still scrolls the list. `start` goes on `onPointerDown` of the element that is the handle; `source`
 * is the element that the user sees move (the row, the header of the category).
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
    const source = (event.currentTarget as HTMLElement).closest<HTMLElement>('[data-drop-row], [data-slot="chat-section-header"]')
    const scroller = source?.closest<HTMLElement>('nav')
    let active = false
    let target: SidebarDrop | null = null
    let ghost: ReturnType<typeof makeGhost> | null = null
    let pointer = { x: startX, y: startY }
    let frame = 0

    const moveGhost = () => {
      if (!ghost) return
      // The ghost stays under the pointer where the row was taken.
      ghost.element.style.transform = `translate(${ghost.rect.left + pointer.x - startX}px, ${ghost.rect.top + pointer.y - startY}px)`
    }
    /** Near the top or the bottom of the list the list scrolls, faster nearer the edge. */
    const autoScroll = () => {
      frame = requestAnimationFrame(autoScroll)
      if (!scroller) return
      const rect = scroller.getBoundingClientRect()
      const above = rect.top + SCROLL_EDGE - pointer.y
      const below = pointer.y - (rect.bottom - SCROLL_EDGE)
      const speed = above > 0 ? -Math.min(above, SCROLL_EDGE) / 3 : below > 0 ? Math.min(below, SCROLL_EDGE) / 3 : 0
      if (speed === 0) return
      const before = scroller.scrollTop
      scroller.scrollTop += speed
      if (scroller.scrollTop !== before) retarget()
    }
    const retarget = () => {
      const next = dropUnder(item, pointer.x, pointer.y)
      if (sameDrop(next, target)) return
      target = next
      setDrop(next)
    }
    const activate = () => {
      active = true
      setDrag(item)
      document.body.style.userSelect = 'none'
      document.body.style.cursor = 'grabbing'
      if (source) ghost = makeGhost(source)
      moveGhost()
      frame = requestAnimationFrame(autoScroll)
    }
    const hold = touch ? setTimeout(activate, HOLD_MS) : undefined

    const end = () => {
      clearTimeout(hold)
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      window.removeEventListener('pointercancel', end)
      window.removeEventListener('touchmove', onTouchMove)
      window.removeEventListener('contextmenu', onContextMenu, true)
      window.removeEventListener('keydown', onKeyDown, true)
      document.body.style.userSelect = ''
      document.body.style.cursor = ''
      cancelAnimationFrame(frame)
      ghost?.element.remove()
      ghost = null
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
      pointer = { x: move.clientX, y: move.clientY }
      moveGhost()
      retarget()
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
    // `Esc` ends the drag and nothing moves. It goes no further: it must not close a pane too.
    const onKeyDown = (key: KeyboardEvent) => {
      if (key.key !== 'Escape' || !active) return
      key.preventDefault()
      key.stopPropagation()
      end()
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
    window.addEventListener('keydown', onKeyDown, true)
    stop.current = end
  }

  return { drag, drop, start }
}
