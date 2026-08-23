import { useEffect, useRef, useState } from 'react'
import { cx } from '../../lib/cx'

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
  'aria-label'?: string
}

/** Coolify `x-forms.listbox`: custom trigger + panel with a check on the selected option. */
export function Listbox<T extends string>({
  id,
  value,
  options,
  onChange,
  placeholder = 'Select…',
  disabled,
  className,
  'aria-label': ariaLabel,
}: ListboxProps<T>) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const current = options.find((option) => option.value === value)

  useEffect(() => {
    if (!open) return
    const onPointerDown = (e: PointerEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false)
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [open])

  return (
    <div ref={rootRef} className={cx('listbox', className)}>
      <button
        type="button"
        id={id}
        className="listbox-trigger"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        title={current?.label}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="listbox-trigger-label">{current?.label ?? placeholder}</span>
        <svg
          className="listbox-trigger-chevron"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          aria-hidden="true"
        >
          <path strokeLinecap="round" strokeLinejoin="round" d="m8 9 4-4 4 4m0 6-4 4-4-4" />
        </svg>
      </button>
      {open ? (
        <div className="listbox-panel" role="listbox">
          {options.length === 0 ? <div className="listbox-empty">No options available.</div> : null}
          {options.map((option) => {
            const selected = option.value === value
            return (
              <button
                key={option.value}
                type="button"
                role="option"
                aria-selected={selected}
                className={cx('listbox-option', option.disabled && 'listbox-option-disabled')}
                onClick={() => {
                  if (option.disabled) return
                  setOpen(false)
                  if (!selected) onChange(option.value)
                }}
              >
                <span className="truncate">{option.label}</span>
                {selected ? (
                  <svg
                    className="listbox-check"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2.5"
                    aria-hidden="true"
                  >
                    <path strokeLinecap="round" strokeLinejoin="round" d="m4.5 12.75 6 6 9-13.5" />
                  </svg>
                ) : null}
              </button>
            )
          })}
        </div>
      ) : null}
    </div>
  )
}
