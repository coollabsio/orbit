import type { ComponentProps, ReactElement, ReactNode } from 'react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

/**
 * A hover/focus tooltip on one control: use it in place of the DOM `title` attribute. The child gets the trigger
 * props, so it must pass its props down to a DOM element. An empty label shows nothing.
 */
export function Tip({ label, side, children }: { label: ReactNode; side?: ComponentProps<typeof TooltipContent>['side']; children: ReactElement }) {
  return (
    <Tooltip disabled={!label}>
      <TooltipTrigger render={children} />
      <TooltipContent side={side}>{label}</TooltipContent>
    </Tooltip>
  )
}
