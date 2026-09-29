import type { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { cn } from 'cn'

/** Actions at the end of a docs tree row (a `group/row`), shown on hover/focus; their clicks never reach the row. */
export function TreeRowActions({ className, onClick, ...props }: ComponentProps<'span'>) {
  return (
    <span
      data-slot="tree-row-actions"
      className={cn(
        'invisible relative z-20 flex shrink-0 items-center gap-0.5 group-hover/row:visible group-focus-within/row:visible',
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

/** The small icon button of a docs tree row or section header (add, options). */
export function TreeIconButton({ className, ...props }: ComponentProps<typeof Button>) {
  return <Button type="button" variant="ghost" size="icon-sm" className={cn('size-[22px] text-muted-foreground/70', className)} {...props} />
}
