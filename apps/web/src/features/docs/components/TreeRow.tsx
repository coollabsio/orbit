import type { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { cn } from 'cn'

/**
 * Actions at the end of a docs tree row (a `group/row`): with a mouse they fade in on hover/focus (and stay while one of
 * their menus is open); on touch they are always shown. Their clicks never reach the row.
 */
export function TreeRowActions({ className, onClick, ...props }: ComponentProps<'span'>) {
  return (
    <span
      data-slot="tree-row-actions"
      className={cn(
        'relative z-20 flex shrink-0 items-center gap-0.5 transition-opacity duration-150 ease-out hover-fine:opacity-0 hover-fine:group-hover/row:opacity-100 hover-fine:group-focus-within/row:opacity-100 hover-fine:has-data-[popup-open]:opacity-100',
        className,
      )}
      onClick={(event) => {
        event.stopPropagation()
        onClick?.(event)
      }}
      {...props}
    />
  )
}

/**
 * The small icon button of a docs tree row or section header (add, options); its hit area reaches past the 22px box.
 * `tip` is its tooltip.
 */
export function TreeIconButton({ className, tip, ...props }: ComponentProps<typeof Button> & { tip?: string }) {
  return (
    <Tip label={tip}>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        className={cn('relative size-[22px] text-muted-foreground/70 after:absolute after:-inset-1', className)}
        {...props}
      />
    </Tip>
  )
}
