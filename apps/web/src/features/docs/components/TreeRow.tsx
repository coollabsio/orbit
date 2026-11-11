import type { ComponentProps } from 'react'
import { Add, DocumentText, Grid } from 'reicon-react'
import type { PageKind } from '@orbit/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Tip } from '@/components/common/Tip'
import { cn } from 'cn'

/**
 * Actions at the end of a docs tree row (a `group/row`). On a mouse they take no width until the row is hovered,
 * focused, or a menu is open, so the title uses the full row. On touch they stay in the row. Clicks do not reach the row.
 */
export function TreeRowActions({ className, onClick, ...props }: ComponentProps<'span'>) {
  return (
    <span
      data-slot="tree-row-actions"
      className={cn(
        'relative z-20 flex shrink-0 items-center gap-0.5 transition-opacity duration-150 ease-out',
        // Idle on a mouse: width 0, so the title is not cut early for buttons the user cannot see.
        'hover-fine:w-0 hover-fine:min-w-0 hover-fine:overflow-hidden hover-fine:opacity-0',
        'hover-fine:group-hover/row:w-auto hover-fine:group-hover/row:min-w-min hover-fine:group-hover/row:overflow-visible hover-fine:group-hover/row:opacity-100',
        'hover-fine:group-focus-within/row:w-auto hover-fine:group-focus-within/row:min-w-min hover-fine:group-focus-within/row:overflow-visible hover-fine:group-focus-within/row:opacity-100',
        'hover-fine:has-data-[popup-open]:w-auto hover-fine:has-data-[popup-open]:min-w-min hover-fine:has-data-[popup-open]:overflow-visible hover-fine:has-data-[popup-open]:opacity-100',
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

/** The "+" of a docs tree row or section header: a menu to create a page or a database there. */
export function TreeCreateMenu({ tip, label, onCreate }: { tip: string; label: string; onCreate: (kind: PageKind) => void }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger render={<TreeIconButton tip={tip} aria-label={label} />}>
        <Add className="size-[13px]" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-auto min-w-36">
        <DropdownMenuItem onClick={() => onCreate('page')}>
          <DocumentText className="size-[14px]" />
          New page
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => onCreate('database')}>
          <Grid className="size-[14px]" />
          New database
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
