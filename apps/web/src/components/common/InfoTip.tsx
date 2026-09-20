import { Info } from 'lucide-react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

/** Small info icon with a hover/focus tooltip. */
export function InfoTip({ text, size = 14 }: { text: string; size?: number }) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <span className="inline-flex cursor-default text-muted-foreground" tabIndex={0} role="img" aria-label={text} />
        }
      >
        <Info size={size} aria-hidden="true" />
      </TooltipTrigger>
      <TooltipContent>{text}</TooltipContent>
    </Tooltip>
  )
}
