import { Check } from 'reicon-react'

/** Checkbox look for a multi-select command row; the row itself carries `aria-checked`. */
export function CheckMark({ checked }: { checked: boolean }) {
  return (
    <span
      aria-hidden="true"
      data-slot="check-mark"
      data-checked={checked || undefined}
      className="flex size-3.5 shrink-0 items-center justify-center rounded-[4px] border border-input text-primary-foreground transition-colors duration-150 data-checked:border-primary data-checked:bg-primary"
    >
      {checked ? <Check className="size-2.5" /> : null}
    </span>
  )
}
