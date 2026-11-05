import { cn } from 'cn'
import { Tip } from '@/components/common/Tip'

/** Coolify `x-application.settings-section`: header strip (title, actions) + body panel below a hairline. Like Coolify, the description shows on hover or focus of the underlined title. */
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
        'm-0 flex w-full min-w-0 flex-col rounded-lg bg-card shadow-[0_0_0_1px_var(--border)]',
        className,
      )}
    >
      <header className="flex min-h-12 flex-wrap items-center justify-between gap-2 rounded-t-lg py-2 pr-2 pl-4 text-sm leading-5 font-medium text-muted-foreground">
        <div className="min-w-0 py-0.5">
          {description ? (
            <>
              <Tip label={description} side="bottom">
                <h3 tabIndex={0} className="w-fit cursor-help text-sm leading-5 font-medium text-foreground underline decoration-muted-foreground decoration-dotted underline-offset-4">{title}</h3>
              </Tip>
              <p className="sr-only">{description}</p>
            </>
          ) : (
            <h3 className="text-sm leading-5 font-medium text-foreground">{title}</h3>
          )}
        </div>
        {actions ? <div className="flex min-w-0 max-w-full flex-wrap items-center gap-2 text-foreground">{actions}</div> : null}
      </header>
      <div
        className={cn(
          'relative min-w-0 rounded-b-lg border-t bg-background',
          flush ? 'overflow-hidden p-0' : 'p-4',
        )}
      >
        {children}
      </div>
    </section>
  )
}
