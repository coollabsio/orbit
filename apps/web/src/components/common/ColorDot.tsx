import { cn } from 'cn'

/** A small round swatch of a project or label colour (the colour is runtime data, so it stays inline). */
export function ColorDot({ color, className }: { color: string | undefined; className?: string }) {
  return <span aria-hidden data-slot="color-dot" className={cn('size-1.5 shrink-0 rounded-full', className)} style={color ? { background: color } : undefined} />
}
