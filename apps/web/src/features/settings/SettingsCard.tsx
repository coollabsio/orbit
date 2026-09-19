import { cn } from 'cn'

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
    <section
      className={cn(
        'm-0 flex w-full min-w-0 flex-col rounded-lg bg-card shadow-[0_0_0_1px_var(--border)] transition-shadow duration-200 hover:shadow-[0_0_0_1px_var(--border),0_8px_24px_rgba(0,0,0,0.08)]',
        className,
      )}
    >
      <header className="flex min-h-12 flex-wrap items-center justify-between gap-2 rounded-t-lg py-2 pr-2 pl-4 text-sm leading-5 font-medium text-muted-foreground">
        <div className="min-w-0 py-0.5">
          <h3 className="text-sm leading-5 font-medium text-muted-foreground">{title}</h3>
          {description ? <p className="mt-0.5 text-xs leading-4 text-muted-foreground">{description}</p> : null}
        </div>
        {actions ? <div className="flex min-w-0 max-w-full flex-wrap items-center gap-2">{actions}</div> : null}
      </header>
      <div
        className={cn(
          'relative min-w-0 rounded-lg bg-background shadow-[0_0_0_1px_var(--muted)]',
          flush ? 'overflow-hidden p-0' : 'p-4',
        )}
      >
        {children}
      </div>
    </section>
  )
}
