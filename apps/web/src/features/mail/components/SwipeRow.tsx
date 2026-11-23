import { useRef, useState, type PointerEvent, type ReactNode } from 'react'
import { Archive, DirectInbox, Reply, ShieldCross, Sms as Mail, SmsTracking, Star, Trash, Undo } from 'reicon-react'
import type { SwipeIcon, SwipeStep } from '@/features/mail/mailSwipe'

const ICONS: Record<SwipeIcon, typeof Archive> = {
  archive: Archive,
  inbox: DirectInbox,
  trash: Trash,
  restore: Undo,
  read: Mail,
  unread: SmsTracking,
  star: Star,
  spam: ShieldCross,
  reply: Reply,
}

/** The part of the row's width a swipe must pass to act. */
const THRESHOLD = 0.3
const SLIDE_MS = 160

type Side = 'left' | 'right'
const sideOf = (dx: number): Side => (dx < 0 ? 'left' : 'right')

/**
 * A list row that slides under a finger and shows the action behind it (touch only: a mouse keeps its hover actions).
 * Past the threshold, letting go runs the action: a row that leaves the folder slides out, any other slides back.
 * `resetKey` changes with the row's data, and puts a row that slid out but stayed back in place.
 */
export function SwipeRow({ left, right, onSwipe, resetKey, children }: { left: SwipeStep | null; right: SwipeStep | null; onSwipe: (step: SwipeStep) => void; resetKey: unknown; children: ReactNode }) {
  const [dx, setDx] = useState(0)
  const [dragging, setDragging] = useState(false)
  const [width, setWidth] = useState(0)
  const gesture = useRef<{ id: number; x: number; y: number; axis: 'x' | 'y' | null } | null>(null)
  // the click that follows a swipe must not open the thread
  const swiped = useRef(false)
  const stepOf = (side: Side) => (side === 'left' ? left : right)

  const [shownKey, setShownKey] = useState(resetKey)
  if (shownKey !== resetKey) {
    setShownKey(resetKey)
    setDx(0)
    setDragging(false)
  }

  const end = (event: PointerEvent<HTMLDivElement>, cancelled: boolean) => {
    const current = gesture.current
    gesture.current = null
    if (!current || current.id !== event.pointerId || current.axis !== 'x') return
    swiped.current = true
    setDragging(false)
    const step = stepOf(sideOf(dx))
    if (cancelled || !step || Math.abs(dx) < width * THRESHOLD) {
      setDx(0)
      return
    }
    if (step.leaves) {
      setDx(Math.sign(dx) * width)
      window.setTimeout(() => onSwipe(step), SLIDE_MS)
    } else {
      setDx(0)
      onSwipe(step)
    }
  }

  const step = dx === 0 ? null : stepOf(sideOf(dx))
  const Icon = step ? ICONS[step.icon] : null
  const past = Math.abs(dx) >= width * THRESHOLD
  return (
    <div
      data-slot="mail-swipe-row"
      className="relative touch-pan-y overflow-hidden"
      onPointerDown={(event) => {
        if (event.pointerType !== 'touch' || gesture.current) return
        swiped.current = false
        setWidth(event.currentTarget.offsetWidth)
        gesture.current = { id: event.pointerId, x: event.clientX, y: event.clientY, axis: null }
      }}
      onPointerMove={(event) => {
        const current = gesture.current
        if (!current || current.id !== event.pointerId) return
        const moveX = event.clientX - current.x
        const moveY = event.clientY - current.y
        if (current.axis === null) {
          if (Math.abs(moveX) < 10 && Math.abs(moveY) < 10) return
          // a clear horizontal move toward a side with an action; anything else stays a scroll or a tap
          current.axis = Math.abs(moveX) > Math.abs(moveY) * 1.5 && stepOf(sideOf(moveX)) ? 'x' : 'y'
          if (current.axis === 'x') {
            event.currentTarget.setPointerCapture(event.pointerId)
            setDragging(true)
          }
        }
        if (current.axis === 'x') setDx(stepOf(sideOf(moveX)) ? moveX : 0)
      }}
      onPointerUp={(event) => end(event, false)}
      onPointerCancel={(event) => end(event, true)}
      onClickCapture={(event) => {
        if (!swiped.current) return
        swiped.current = false
        event.stopPropagation()
        event.preventDefault()
      }}
    >
      {step && Icon ? (
        <div
          aria-hidden
          className="absolute inset-0 flex items-center gap-2 px-5 text-[0.8125rem] font-medium text-white transition-opacity"
          style={{ backgroundColor: step.color, opacity: past ? 1 : 0.6, justifyContent: dx < 0 ? 'flex-end' : 'flex-start' }}
        >
          <Icon className="size-4.5" />
          {step.label}
        </div>
      ) : null}
      <div className="relative bg-background" style={{ transform: dx ? `translateX(${dx}px)` : undefined, transition: dragging ? 'none' : `transform ${SLIDE_MS}ms ease-out` }}>
        {children}
      </div>
    </div>
  )
}
