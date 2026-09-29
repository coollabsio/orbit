import type { ComponentProps } from 'react'
import { cva } from 'class-variance-authority'
import { ContextMenuItem } from '@/components/ui/context-menu'
import { DropdownMenuItem } from '@/components/ui/dropdown-menu'
import { cn } from 'cn'

// data-danger (not variant="destructive"): the preset menu popup forces destructive items to the accent color.
// The `!` beats the item's focus rule that recolors every descendant to accent-foreground.
const chatMenuItemVariants = cva('', {
  variants: {
    danger: {
      true: 'text-destructive focus:bg-destructive/10 focus:text-destructive **:text-destructive!',
      false: '',
    },
  },
  defaultVariants: { danger: false },
})

type DangerProps = { danger?: boolean }

/** shadcn DropdownMenuItem with a red `danger` state for destructive actions. */
export function ChatDropdownMenuItem({ danger = false, className, ...props }: ComponentProps<typeof DropdownMenuItem> & DangerProps) {
  return <DropdownMenuItem data-danger={danger || undefined} className={cn(chatMenuItemVariants({ danger }), className)} {...props} />
}

/** shadcn ContextMenuItem with a red `danger` state for destructive actions. */
export function ChatContextMenuItem({ danger = false, className, ...props }: ComponentProps<typeof ContextMenuItem> & DangerProps) {
  return <ContextMenuItem data-danger={danger || undefined} className={cn(chatMenuItemVariants({ danger }), className)} {...props} />
}
