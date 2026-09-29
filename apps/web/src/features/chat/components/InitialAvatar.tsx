import type { ComponentProps } from 'react'
import { cn } from 'cn'

/** Round first-letter avatar tinted with the author's color. Size and text size come from `className`. */
export function InitialAvatar({ name, color, className, ...props }: ComponentProps<'span'> & { name: string; color?: string }) {
  return (
    <span
      data-slot="initial-avatar"
      className={cn('flex shrink-0 items-center justify-center overflow-hidden rounded-full bg-muted font-semibold text-muted-foreground', className)}
      style={color ? { background: `color-mix(in srgb, ${color} 22%, transparent)`, color } : undefined}
      {...props}
    >
      {name.charAt(0).toUpperCase()}
    </span>
  )
}
