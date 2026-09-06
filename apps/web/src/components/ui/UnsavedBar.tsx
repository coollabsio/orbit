import { useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

/** Desktop pane footer; mobile top-of-screen overlay. */
export function UnsavedBar({ onReset, onSave, saving }: { onReset: () => void; onSave: () => void; saving?: boolean }) {
  const barRef = useRef<HTMLDivElement>(null)
  const [mobile, setMobile] = useState(() => window.matchMedia('(max-width: 899px)').matches)

  useLayoutEffect(() => {
    const media = window.matchMedia('(max-width: 899px)')
    const update = () => setMobile(media.matches)
    media.addEventListener('change', update)
    return () => media.removeEventListener('change', update)
  }, [])

  useLayoutEffect(() => {
    const bar = barRef.current
    const viewport = window.visualViewport
    if (!mobile || !bar || !viewport) return

    // iOS pans and zooms the visual viewport independently of the layout viewport.
    const update = () => {
      bar.style.top = `calc(${viewport.offsetTop + 12}px + env(safe-area-inset-top, 0px))`
      bar.style.left = `${viewport.offsetLeft + viewport.width / 2}px`
      bar.style.width = `${Math.max(0, viewport.width - 24)}px`
    }
    update()
    viewport.addEventListener('resize', update)
    viewport.addEventListener('scroll', update)
    return () => {
      viewport.removeEventListener('resize', update)
      viewport.removeEventListener('scroll', update)
    }
  }, [mobile])

  const bar = (
    <div ref={barRef} className="unsaved-bar" role="status" style={mobile ? { position: 'fixed', top: 'calc(12px + env(safe-area-inset-top, 0px))', bottom: 'auto', width: 'calc(100% - 24px)', animation: 'none' } : undefined}>
      <span className="unsaved-bar-text">Careful — you have unsaved changes!</span>
      <button type="button" className="unsaved-bar-reset" onClick={onReset}>
        Reset
      </button>
      <button type="button" className="button button-primary" onClick={onSave} disabled={saving}>
        Save Changes
      </button>
    </div>
  )
  return mobile ? createPortal(bar, document.body) : bar
}
