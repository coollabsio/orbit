import { cn } from 'cn'
import type { MemberStatus } from './memberStatus'

/**
 * The presence mark. Each status has its own shape, so it does not depend on colour alone: online is a filled dot,
 * idle a crescent, do not disturb a dot with a bar, offline and invisible a hollow ring. The cut-outs take the colour
 * of the surface behind (`bg-background`).
 */
export function StatusMark({ status, size, className }: { status: MemberStatus; size: number; className?: string }) {
  return (
    <span
      data-slot="status-mark"
      data-status={status}
      aria-hidden="true"
      className={cn(
        'relative inline-flex shrink-0 items-center justify-center overflow-hidden rounded-full border-[1.5px] border-muted-foreground bg-background data-[status=dnd]:border-0 data-[status=dnd]:bg-red-500 data-[status=idle]:border-0 data-[status=idle]:bg-amber-500 data-[status=online]:border-0 data-[status=online]:bg-green-500',
        className,
      )}
      style={{ width: size, height: size }}
    >
      {status === 'idle' ? <span className="absolute -top-[15%] -left-[15%] size-[65%] rounded-full bg-background" /> : null}
      {status === 'dnd' ? <span className="h-[22%] w-[60%] rounded-full bg-background" /> : null}
    </span>
  )
}
