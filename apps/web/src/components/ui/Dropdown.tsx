import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { dropdownPosition } from './dropdownPosition'
import { cx } from '../../lib/cx'

interface DropdownProps {
  /** Render the trigger. `open` is the current state. */
  trigger: (open: boolean) => React.ReactNode
  /** Render the menu content. Call `close` after an option is picked. */
  children: (close: () => void) => React.ReactNode
  align?: 'left' | 'right'
  /** Preferred opening direction; flips when the other side has more space. */
  direction?: 'down' | 'up'
  className?: string
}

/** Simple popover dropdown: opens next to the trigger, closes on outside click / Escape. */
export function Dropdown({ trigger, children, align = 'left', direction = 'down', className }: DropdownProps) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)

  const panelRef = useRef<HTMLDivElement>(null)

  useLayoutEffect(() => {
    if (!open) return
    const panel = panelRef.current
    const root = rootRef.current
    if (!panel || !root) return
    const update = () => {
      const viewport = window.visualViewport
      const dock = document.querySelector('.mobile-dock')?.getBoundingClientRect()
      const bottom = (viewport?.offsetTop ?? 0) + (viewport?.height ?? window.innerHeight)
      // Measure at the CSS height cap before applying the available viewport space.
      const scrollTop = panel.scrollTop
      panel.style.maxHeight = ''
      const height = panel.offsetHeight
      const width = panel.offsetWidth
      const next = dropdownPosition(root.getBoundingClientRect(), { width, height }, {
        top: viewport?.offsetTop ?? 0,
        bottom: dock && dock.height > 0 ? Math.min(bottom, dock.top) : bottom,
        left: viewport?.offsetLeft ?? 0,
        width: viewport?.width ?? window.innerWidth,
      }, align, direction)
      Object.assign(panel.style, {
        top: `${next.top}px`,
        left: `${next.left}px`,
        maxHeight: `${Math.max(0, Math.min(height, next.maxHeight))}px`,
        visibility: 'visible',
      })
      panel.scrollTop = scrollTop
    }
    update()
    window.addEventListener('resize', update)
    const onScroll = (event: Event) => {
      if (!panel.contains(event.target as Node)) update()
    }
    document.addEventListener('scroll', onScroll, true)
    window.visualViewport?.addEventListener('resize', update)
    window.visualViewport?.addEventListener('scroll', update)
    const observer = new ResizeObserver(update)
    observer.observe(root)
    // Content can change while a multi-select menu stays open.
    const mutations = new MutationObserver(update)
    mutations.observe(panel, { childList: true, subtree: true, characterData: true })
    return () => {
      window.removeEventListener('resize', update)
      document.removeEventListener('scroll', onScroll, true)
      window.visualViewport?.removeEventListener('resize', update)
      window.visualViewport?.removeEventListener('scroll', update)
      observer.disconnect()
      mutations.disconnect()
    }
  }, [open, align, direction])

  useEffect(() => {
    if (!open) return
    const onPointerDown = (e: PointerEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node) && !panelRef.current?.contains(e.target as Node)) setOpen(false)
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
    <div ref={rootRef} style={{ position: 'relative', display: 'inline-flex' }} className={className} data-dropdown-open={open || undefined}>
      <div style={{ display: 'inline-flex' }} onClick={() => setOpen((o) => !o)}>
        {trigger(open)}
      </div>
      {open ? createPortal(
        <div className={className} style={{ display: 'contents' }}>
          <div
            ref={panelRef}
            className={cx('popover')}
            style={{
              position: 'fixed',
              zIndex: 200,
              overflowY: 'auto',
              visibility: 'hidden',
            }}
          >
            {children(() => setOpen(false))}
          </div>
        </div>, document.body
      ) : null}
    </div>
  )
}
