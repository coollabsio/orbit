import { useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'

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
    <div
      ref={barRef}
      role="status"
      className={cn(
        'absolute bottom-5 left-1/2 z-40 flex w-[min(720px,calc(100%-48px))] -translate-x-1/2 items-center gap-3 rounded-lg border bg-popover py-2.5 pr-2.5 pl-4 shadow-md',
        mobile && 'fixed top-[calc(12px+env(safe-area-inset-top,0px))] bottom-auto w-[calc(100%-24px)]',
      )}
    >
      <span className="min-w-0 flex-1 text-[13px] font-medium">Careful — you have unsaved changes!</span>
      <Button variant="link" className="text-primary" onClick={onReset}>
        Reset
      </Button>
      <Button onClick={onSave} disabled={saving}>
        Save Changes
      </Button>
    </div>
  )
  return mobile ? createPortal(bar, document.body) : bar
}
