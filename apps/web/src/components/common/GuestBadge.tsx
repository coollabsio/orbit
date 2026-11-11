import { Badge } from '@/components/ui/badge'
import { cn } from 'cn'

/** Marks a guest: an outside person who is only in some private channels. */
export function GuestBadge({ className }: { className?: string }) {
  return (
    <Badge data-slot="guest-badge" variant="outline" className={cn('h-4 px-1.5 text-[10px] text-muted-foreground', className)}>
      Guest
    </Badge>
  )
}
