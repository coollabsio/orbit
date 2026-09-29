import type { ComponentProps } from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { Button } from '@/components/ui/button'
import { cn } from 'cn'

const chatIconButtonVariants = cva(
  'text-muted-foreground data-[active=true]:text-primary data-[active=true]:hover:bg-primary/10 dark:data-[active=true]:hover:bg-primary/10',
  {
    variants: {
      size: {
        default: 'size-8',
        sm: 'size-7',
        /** Chat header: larger icons, shrinks on mobile. */
        header: "size-8 [&_svg:not([class*='size-'])]:size-5 max-[899px]:size-[30px] max-[899px]:[&_svg:not([class*='size-'])]:size-[17px]",
      },
    },
    defaultVariants: { size: 'default' },
  },
)

/** Muted ghost icon button for chat toolbars; `active` tints it with the primary color (open panel, followed thread). */
export function ChatIconButton({
  active = false,
  size = 'default',
  className,
  ...props
}: Omit<ComponentProps<typeof Button>, 'size' | 'variant'> & VariantProps<typeof chatIconButtonVariants> & { active?: boolean }) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      data-slot="chat-icon-button"
      data-active={active || undefined}
      className={cn(chatIconButtonVariants({ size }), className)}
      {...props}
    />
  )
}
