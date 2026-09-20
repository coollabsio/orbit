import type { ComponentType, ReactNode } from 'react'
import { cn } from 'cn'
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from '@/components/ui/empty'

interface EmptyStateProps {
  icon: ComponentType<{ size?: number; className?: string }>
  title: string
  description?: string
  action?: ReactNode
  size?: 'sm' | 'base'
}

/** App empty state: dashed card with an icon tile, title, description, optional actions. */
export function EmptyState({ icon: Icon, title, description, action, size = 'base' }: EmptyStateProps) {
  return (
    <Empty className={cn('border', size === 'sm' ? 'min-h-44 p-4' : 'min-h-80')}>
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <Icon className={size === 'sm' ? 'size-4.5' : 'size-5'} />
        </EmptyMedia>
        <EmptyTitle>{title}</EmptyTitle>
        {description ? <EmptyDescription>{description}</EmptyDescription> : null}
      </EmptyHeader>
      {action ? <EmptyContent>{action}</EmptyContent> : null}
    </Empty>
  )
}
