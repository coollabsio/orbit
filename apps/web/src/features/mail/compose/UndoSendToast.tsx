import { useEffect, useState, useSyncExternalStore } from 'react'
import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'

/** The toast's content: what happened, **Undo (Z)**, and the bar that runs out when the message goes. */
export function UndoSendToast({ title, countdownMs, onUndo }: { title: string; countdownMs: number | null; onUndo: () => void }) {
  const reducedMotion = useSyncExternalStore(subscribeReducedMotion, prefersReducedMotion)
  return (
    <div
      data-slot="undo-send-toast"
      className="relative flex w-(--width) items-center gap-3 overflow-hidden rounded-lg border bg-popover py-2.5 pr-2.5 pl-4 text-[0.8125rem] text-popover-foreground shadow-lg"
    >
      <span role="status" className="min-w-0 flex-1 truncate font-medium">
        {title}
      </span>
      {/* reduced motion: the seconds left, as text, in place of the moving bar */}
      {countdownMs !== null && reducedMotion ? <Countdown ms={countdownMs} /> : null}
      <Button size="sm" variant="ghost" className="shrink-0 gap-1.5" aria-keyshortcuts="Z" aria-label="Undo (Z)" onClick={onUndo}>
        Undo
        <Kbd>Z</Kbd>
      </Button>
      {countdownMs !== null ? (
        // linear: it is a clock, not an object moving
        <span
          aria-hidden="true"
          className="absolute inset-x-0 bottom-0 h-0.5 origin-left animate-undo-countdown bg-primary/70 motion-reduce:hidden"
          style={{ animationDuration: `${countdownMs}ms` }}
        />
      ) : null}
    </div>
  )
}

const REDUCED_MOTION = '(prefers-reduced-motion: reduce)'
const prefersReducedMotion = () => window.matchMedia(REDUCED_MOTION).matches
const subscribeReducedMotion = (onChange: () => void) => {
  const media = window.matchMedia(REDUCED_MOTION)
  media.addEventListener('change', onChange)
  return () => media.removeEventListener('change', onChange)
}

/** "8 s": the seconds before the message goes, with reduced motion only (the bar shows it otherwise). */
function Countdown({ ms }: { ms: number }) {
  const [end] = useState(() => Date.now() + ms)
  const [left, setLeft] = useState(() => Math.ceil(ms / 1000))
  useEffect(() => {
    const timer = setInterval(() => setLeft(Math.max(0, Math.ceil((end - Date.now()) / 1000))), 250)
    return () => clearInterval(timer)
  }, [end])
  return (
    <span aria-hidden="true" className="shrink-0 text-xs text-muted-foreground tabular-nums">
      {left} s
    </span>
  )
}
