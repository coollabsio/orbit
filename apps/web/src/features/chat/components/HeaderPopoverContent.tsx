import type { ComponentProps } from 'react'
import { PopoverContent } from '@/components/ui/popover'
import { cn } from 'cn'

/** Wide panel (threads, pins) that drops from the right edge of the chat header; full width on mobile. */
export function HeaderPopoverContent({ className, ...props }: ComponentProps<typeof PopoverContent>) {
  return (
    <PopoverContent
      side="bottom"
      align="end"
      sideOffset={0}
      alignOffset={16}
      className={cn(
        'max-h-[78vh] w-[544px] max-w-[calc(100vw-32px)] gap-0 p-0 max-[899px]:max-h-[calc(100dvh-108px)] max-[899px]:w-[calc(100vw-16px)] max-[899px]:max-w-none',
        className,
      )}
      {...props}
    />
  )
}
