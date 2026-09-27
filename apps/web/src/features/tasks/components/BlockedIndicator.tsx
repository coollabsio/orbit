import { Flag } from 'reicon-react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from 'cn'

/** Orange flag (Linear style) when an open task blocks this one; `pill` wraps it like the other property chips. Static: no motion. */
export function BlockedIndicator({ pill = false, size = pill ? 10 : 12 }: { pill?: boolean; size?: number }) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <span
            role="img"
            aria-label="Blocked"
            className={cn('inline-flex shrink-0 items-center text-orange-500', pill && 'size-[18px] justify-center rounded-full bg-muted')}
          />
        }
      >
        <Flag size={size} aria-hidden="true" />
      </TooltipTrigger>
      <TooltipContent>Blocked</TooltipContent>
    </Tooltip>
  )
}
