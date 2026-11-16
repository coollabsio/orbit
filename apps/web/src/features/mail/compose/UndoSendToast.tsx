import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'

/** The toast's content: what happened, **Undo (Z)**, and the bar that runs out when the message goes. */
export function UndoSendToast({ title, countdownMs, onUndo }: { title: string; countdownMs: number | null; onUndo: () => void }) {
  return (
    <div
      data-slot="undo-send-toast"
      className="relative flex w-(--width) items-center gap-3 overflow-hidden rounded-lg border bg-popover py-2.5 pr-2.5 pl-4 text-[0.8125rem] text-popover-foreground shadow-lg"
    >
      <span role="status" className="min-w-0 flex-1 truncate font-medium">
        {title}
      </span>
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
