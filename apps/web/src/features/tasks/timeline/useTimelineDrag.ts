import { useRef, useState, type PointerEvent as ReactPointerEvent, type RefObject } from 'react'
import { dayIndexAtX, type DragMode } from './timelineLib'

export type DragKind = DragMode | 'draw'

export interface DragState {
  taskId: string
  kind: DragKind
  /** Pointer x at press, in track (content) pixels. */
  originX: number
  deltaDays: number
  moved: boolean
}

const CLICK_SLOP = 4
const EDGE = 40
const EDGE_SPEED = 12

/** Pointer drag on the timeline track: day-snapped deltas, edge auto-scroll, Esc to cancel. */
export function useTimelineDrag({ pxPerDay, trackRef, scrollRef, onCommit }: {
  pxPerDay: number
  trackRef: RefObject<HTMLElement | null>
  scrollRef: RefObject<HTMLElement | null>
  onCommit: (state: DragState) => void
}) {
  const [drag, setDrag] = useState<DragState | null>(null)
  const suppressClick = useRef(false)

  const begin = (event: ReactPointerEvent, taskId: string, kind: DragKind) => {
    if (event.button !== 0 || event.pointerType === 'touch') return
    const track = trackRef.current
    if (!track) return
    // the track moves with scroll, so its rect turns client x into content x
    const trackX = (clientX: number) => clientX - track.getBoundingClientRect().left
    const startClientX = event.clientX
    let lastClientX = event.clientX
    let state: DragState = { taskId, kind, originX: trackX(event.clientX), deltaDays: 0, moved: false }
    let frame = 0

    const update = () => {
      const x = trackX(lastClientX)
      const moved = state.moved || Math.abs(lastClientX - startClientX) > CLICK_SLOP
      const deltaDays = !moved ? 0 : kind === 'draw'
        ? dayIndexAtX(x, pxPerDay) - dayIndexAtX(state.originX, pxPerDay)
        : Math.round((x - state.originX) / pxPerDay)
      if (moved !== state.moved || deltaDays !== state.deltaDays) {
        state = { ...state, moved, deltaDays }
        setDrag(state)
      }
    }
    const autoScroll = () => {
      const scroller = scrollRef.current
      if (scroller && state.moved) {
        const rect = scroller.getBoundingClientRect()
        const step = lastClientX < rect.left + EDGE ? -EDGE_SPEED : lastClientX > rect.right - EDGE ? EDGE_SPEED : 0
        if (step !== 0 && rect.width > 0) {
          scroller.scrollLeft += step
          update()
        }
      }
      frame = requestAnimationFrame(autoScroll)
    }
    const finish = (commit: boolean) => {
      cancelAnimationFrame(frame)
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      window.removeEventListener('keydown', onKey)
      setDrag(null)
      if (!commit) return
      if (state.moved) suppressClick.current = true
      if (state.moved || kind === 'draw') onCommit(state)
    }
    const onMove = (move: PointerEvent) => { lastClientX = move.clientX; update() }
    const onUp = (up: PointerEvent) => { lastClientX = up.clientX; update(); finish(true) }
    const onKey = (key: KeyboardEvent) => { if (key.key === 'Escape') finish(false) }

    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
    window.addEventListener('keydown', onKey)
    frame = requestAnimationFrame(autoScroll)
    setDrag(state)
  }

  /** True once after a real drag; the browser still fires click on release. */
  const consumeClick = () => {
    const suppressed = suppressClick.current
    suppressClick.current = false
    return suppressed
  }

  return { drag, begin, consumeClick }
}
