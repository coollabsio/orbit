import { Button as ButtonPrimitive } from '@base-ui/react/button'
import { cn } from 'cn'

/** Unstyled full-width list row (channel, DM, search result, thread card): a button that keeps its own layout,
    like shadcn's sidebar-menu-button. Callers add padding, radius and hover colors. */
export function RowButton({ className, type = 'button', ...props }: Omit<ButtonPrimitive.Props, 'className'> & { className?: string }) {
  return (
    <ButtonPrimitive
      type={type}
      data-slot="row-button"
      className={cn(
        'flex w-full min-w-0 items-center gap-2 text-left outline-none transition-colors select-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0',
        className,
      )}
      {...props}
    />
  )
}
