import type { IconComponent } from 'reicon-react'

interface EmptyStateProps {
  icon: IconComponent
  title: string
  description?: string
  action?: React.ReactNode
  /** Coolify `x-empty` size: sm (min 176px) or base (min 320px). */
  size?: 'sm' | 'base'
}

/** Coolify `x-empty`: dashed card with an icon tile, title, description, optional actions. */
export function EmptyState({ icon: Icon, title, description, action, size = 'base' }: EmptyStateProps) {
  return (
    <div className="empty-state-wrap">
      <div className="empty-state" data-size={size}>
        <div className="empty-state-icon">
          <Icon size={size === 'sm' ? 18 : 20} />
        </div>
        <h2>{title}</h2>
        {description ? <p>{description}</p> : null}
        {action ? <div className="empty-state-actions">{action}</div> : null}
      </div>
    </div>
  )
}
