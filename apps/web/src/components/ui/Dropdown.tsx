import { useEffect, useRef, useState } from 'react'
import { cx } from '../../lib/cx'

interface DropdownProps {
  /** Render the trigger. `open` is the current state. */
  trigger: (open: boolean) => React.ReactNode
  /** Render the menu content. Call `close` after an option is picked. */
  children: (close: () => void) => React.ReactNode
  align?: 'left' | 'right'
  /** Open below (default) or above the trigger. */
  direction?: 'down' | 'up'
  className?: string
}

/** Simple popover dropdown: opens next to the trigger, closes on outside click / Escape. */
export function Dropdown({ trigger, children, align = 'left', direction = 'down', className }: DropdownProps) {
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
            [direction === 'down' ? 'top' : 'bottom']: 'calc(100% + 4px)',
            [align === 'left' ? 'left' : 'right']: 0,
          }}
        >
          {children(() => setOpen(false))}
        </div>
      ) : null}
    </div>
  )
}
