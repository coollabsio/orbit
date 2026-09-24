import { Forbidden2 as Blocked } from 'reicon-react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

/** Small red mark next to a task identifier when an open task blocks it. Static: no motion. */
export function BlockedIndicator({ size = 12 }: { size?: number }) {
  return (
    <Tooltip>
      <TooltipTrigger render={<span role="img" aria-label="Blocked" className="inline-flex shrink-0 text-destructive" />}>
        <Blocked size={size} aria-hidden="true" />
      </TooltipTrigger>
      <TooltipContent>Blocked</TooltipContent>
    </Tooltip>
  )
}
