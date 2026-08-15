import { useEffect, useRef, useState } from 'react'
import { cx } from '../../lib/cx'

interface DropdownProps {
  /** Render the trigger. `open` is the current state. */
  trigger: (open: boolean) => React.ReactNode
  /** Render the menu content. Call `close` after an option is picked. */
  children: (close: () => void) => React.ReactNode
  align?: 'left' | 'right'
  className?: string
}

/** Simple popover dropdown: opens below the trigger, closes on outside click / Escape. */
export function Dropdown({ trigger, children, align = 'left', className }: DropdownProps) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)

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
    <div ref={rootRef} style={{ position: 'relative', display: 'inline-flex' }} className={className}>
      <div style={{ display: 'inline-flex' }} onClick={() => setOpen((o) => !o)}>
        {trigger(open)}
      </div>
      {open ? (
        <div
          className={cx('popover')}
          style={{
            top: 'calc(100% + 4px)',
            [align === 'left' ? 'left' : 'right']: 0,
          }}
        >
          {children(() => setOpen(false))}
        </div>
      ) : null}
    </div>
  )
}
