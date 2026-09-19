import { useState, type ReactElement, type ReactNode } from 'react'
import { cn } from 'cn'
import { Popover, PopoverContent, PopoverTrigger } from './popover'

interface DropdownProps {
  /** Render the trigger. `open` is the current state. Must return a single element. */
  trigger: (open: boolean) => ReactNode
  /** Render the menu content. Call `close` after an option is picked. */
  children: (close: () => void) => ReactNode
  align?: 'left' | 'right'
  /** Preferred opening direction; Base UI flips on collision. */
  direction?: 'down' | 'up'
  className?: string
}

/** Anchored popover: opens next to the trigger, closes on outside click / Escape.
    Built on the shadcn/Base UI Popover. The trigger element is used directly (Base UI
    merges the trigger props onto it), so it keeps its own semantics with no extra wrapper. */
export function Dropdown({ trigger, children, align = 'left', direction = 'down', className }: DropdownProps) {
  const [open, setOpen] = useState(false)
  return (
    <Popover open={open} onOpenChange={setOpen} modal={false}>
      <PopoverTrigger render={trigger(open) as ReactElement} />
      {open ? (
        <PopoverContent
          align={align === 'right' ? 'end' : 'start'}
          side={direction === 'up' ? 'top' : 'bottom'}
          className={cn('max-h-(--available-height) w-auto overflow-y-auto p-0', className)}
        >
          {children(() => setOpen(false))}
        </PopoverContent>
      ) : null}
    </Popover>
  )
}
