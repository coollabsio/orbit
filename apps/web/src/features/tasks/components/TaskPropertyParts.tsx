import type { ComponentProps, ReactNode } from 'react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'

/** A titled block of the properties sidebar; on phones the sidebar becomes a two-column grid of these. */
export function PropertyGroup({ title, className, children }: { title: string; className?: string; children: ReactNode }) {
  return (
    <div data-slot="property-group" className={cn('mb-6 flex flex-col items-start gap-0.5 max-[899px]:mb-0 max-[899px]:min-w-0', className)}>
      <h4 data-slot="property-heading" className="mb-1.5 text-xs font-medium text-muted-foreground/70">{title}</h4>
      {children}
    </div>
  )
}

/** A sidebar property value that opens its editor: a ghost button pulled left so its text lines up with the heading. */
export function PropertyButton({ className, ...props }: ComponentProps<typeof Button>) {
  return (
    <Button
      variant="ghost"
      className={cn('-ml-2 text-[0.8125rem] max-[899px]:h-7 max-[899px]:max-w-full max-[899px]:px-1.5 max-[899px]:text-xs', className)}
      {...props}
    />
  )
}
