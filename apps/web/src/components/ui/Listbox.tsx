import { Check, ChevronsUpDown } from 'lucide-react'
import { cn } from 'cn'
import { Dropdown } from './Dropdown'

export interface ListboxOption<T extends string> {
  value: T
  label: string
  disabled?: boolean
}

interface ListboxProps<T extends string> {
  id?: string
  value: T
  options: ListboxOption<T>[]
  onChange: (value: T) => void
  placeholder?: string
  disabled?: boolean
  className?: string
  selectedValues?: T[]
  displayValue?: string
  closeOnSelect?: boolean
  'aria-label'?: string
}

/** Select-style picker: single choice, or multi-select via `selectedValues` + `closeOnSelect={false}`.
    Built on the Base UI Popover (Dropdown); the panel uses a listbox role with a check on each choice. */
export function Listbox<T extends string>({
  id,
  value,
  options,
  onChange,
  placeholder = 'Select…',
  disabled,
  className,
  selectedValues,
  displayValue,
  closeOnSelect = true,
  'aria-label': ariaLabel,
}: ListboxProps<T>) {
  const current = options.find((option) => option.value === value)
  const label = displayValue ?? current?.label
  return (
    <Dropdown
      className={cn('min-w-(--anchor-width) p-1', className)}
      trigger={(open) => (
        <button
          type="button"
          id={id}
          disabled={disabled}
          aria-haspopup="listbox"
          aria-expanded={open}
          aria-label={ariaLabel}
          title={label}
          className="flex h-9 w-full items-center justify-between gap-2 rounded-md border border-input bg-background px-3 text-sm whitespace-nowrap shadow-xs outline-none hover:bg-muted focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <span className={cn('truncate', !label && 'text-muted-foreground')}>{label ?? placeholder}</span>
          <ChevronsUpDown className="size-4 shrink-0 opacity-50" aria-hidden="true" />
        </button>
      )}
    >
      {(close) => (
        <div role="listbox" aria-multiselectable={selectedValues ? true : undefined} className="flex flex-col">
          {options.length === 0 ? (
            <div className="px-2 py-1.5 text-sm text-muted-foreground">No options available.</div>
          ) : null}
          {options.map((option) => {
            const selected = selectedValues?.includes(option.value) ?? option.value === value
            return (
              <button
                key={option.value}
                type="button"
                role="option"
                aria-selected={selected}
                disabled={option.disabled}
                className="flex cursor-default items-center justify-between gap-2 rounded-sm px-2 py-1.5 text-left text-sm outline-none hover:bg-accent hover:text-accent-foreground disabled:pointer-events-none disabled:opacity-50"
                onClick={() => {
                  if (option.disabled) return
                  if (closeOnSelect) close()
                  if (!selected || !closeOnSelect) onChange(option.value)
                }}
              >
                <span className="truncate">{option.label}</span>
                {selected ? <Check className="size-4 shrink-0" aria-hidden="true" /> : null}
              </button>
            )
          })}
        </div>
      )}
    </Dropdown>
  )
}
