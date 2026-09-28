import { cn } from 'cn'

/** "Add as sub-issue" over a row or card while a drop would nest into it: 150ms fade + scale(0.97 → 1), reduced motion fades. */
export function NestChip({ className }: { className?: string }) {
  return (
    <span aria-hidden className={cn('pointer-events-none absolute z-[2] animate-filter-chip-enter rounded-full bg-primary px-2 py-0.5 text-[11px] leading-4 font-medium whitespace-nowrap text-primary-foreground shadow-sm', className)}>
      Add as sub-issue
    </span>
  )
}
