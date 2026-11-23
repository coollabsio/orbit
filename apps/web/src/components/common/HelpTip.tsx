import type { ReactElement } from 'react'
import { Popover as PopoverPrimitive } from '@base-ui/react/popover'

/**
 * Help text on a non-button element: it opens on hover or focus like a tooltip, and also on a tap or click, because a
 * tooltip never opens on a touch screen.
 */
export function HelpTip({ text, side = 'top', children }: { text: string; side?: 'top' | 'bottom'; children: ReactElement }) {
  return (
    <PopoverPrimitive.Root>
      <PopoverPrimitive.Trigger openOnHover delay={100} nativeButton={false} render={children} />
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Positioner side={side} sideOffset={4} collisionPadding={8} className="isolate z-[110]">
          <PopoverPrimitive.Popup
            initialFocus={false}
            className="w-fit max-w-xs rounded-md border bg-popover px-3 py-1.5 text-xs text-popover-foreground shadow-md outline-hidden"
          >
            {text}
          </PopoverPrimitive.Popup>
        </PopoverPrimitive.Positioner>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  )
}
