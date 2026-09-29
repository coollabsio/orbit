import type { ReactNode } from 'react'

/** Chat settings tab intro: title, description and the tab's primary action. */
export function SettingsTabHeader({ title, description, children }: { title: string; description: ReactNode; children?: ReactNode }) {
  return (
    <div data-slot="settings-tab-header" className="flex flex-col items-start gap-4 border-b border-border pb-7">
      <div>
        <h2 className="text-xl leading-7 font-semibold text-foreground">{title}</h2>
        <p className="mt-3 max-w-[576px] text-sm leading-6 text-foreground">{description}</p>
      </div>
      {children}
    </div>
  )
}

/** Inline error banner under a settings tab header. */
export function SettingsTabError({ children }: { children: ReactNode }) {
  return (
    <p role="alert" data-slot="settings-tab-error" className="rounded-lg bg-destructive/10 px-3 py-2 text-sm text-destructive">
      {children}
    </p>
  )
}
