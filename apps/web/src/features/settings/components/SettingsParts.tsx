import type { ComponentProps } from 'react'
import { cn } from 'cn'

/** Settings form fields: one column on phones, two from 900px. */
function FieldGrid({ className, ...props }: ComponentProps<'div'>) {
  return <div data-slot="field-grid" className={cn('grid grid-cols-1 gap-4 min-[900px]:grid-cols-2', className)} {...props} />
}

/** The pink asterisk after a required field's label. */
function RequiredMark() {
  return <span data-slot="required-mark" className="font-semibold text-primary">*</span>
}

/** One row of a flush settings card (a list of labels, backups, …). */
function SettingsRow({ className, ...props }: ComponentProps<'div'>) {
  return <div data-slot="settings-row" className={cn('flex flex-wrap items-center gap-3 px-4 py-3', className)} {...props} />
}

/** The square icon tile at the start of a settings row. */
function RowIcon({ className, ...props }: ComponentProps<'span'>) {
  return <span data-slot="row-icon" className={cn('inline-flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground [&_svg]:size-4.5', className)} {...props} />
}

export { FieldGrid, RequiredMark, RowIcon, SettingsRow }
