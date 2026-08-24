import { cx } from '../../lib/cx'

/** Coolify `x-application.settings-section`: header strip (title, description, actions) + nested body panel. */
export function SettingsCard({
  title,
  description,
  actions,
  flush,
  className,
  children,
}: {
  title: string
  description?: string
  actions?: React.ReactNode
  flush?: boolean
  className?: string
  children: React.ReactNode
}) {
  return (
    <section className={cx('layer-card', className)}>
      <header className="layer-card-header">
        <div className="layer-card-heading">
          <h3>{title}</h3>
          {description ? <p>{description}</p> : null}
        </div>
        {actions ? <div className="layer-card-actions">{actions}</div> : null}
      </header>
      <div className={cx('layer-card-body', flush && 'flush')}>{children}</div>
    </section>
  )
}
