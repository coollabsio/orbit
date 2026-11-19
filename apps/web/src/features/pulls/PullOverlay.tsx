import { Dialog } from '@base-ui/react/dialog'
import { pullParam } from '@orbit/api/github'
import { useCallback, useMemo, useState } from 'react'
import { Navigate, useLocation, useNavigate, useParams } from 'react-router'
import { PullReview } from '@/features/pulls/components/PullReview'
import { pullFromSearch, withoutPull, withPull } from '@/features/pulls/lib/overlay'
import '@/features/pulls/pulls.css'

/** The focused element, also inside a shadow root (the diffs render in one). */
function focused(): Element | null {
  let element = document.activeElement
  while (element?.shadowRoot?.activeElement) element = element.shadowRoot.activeElement
  return element
}

/** A pull request over whatever page opened it (`?pr=owner/repo/n`). Esc and Back close it; the page under it stays. */
export function PullOverlay() {
  const location = useLocation()
  const navigate = useNavigate()
  const opened = useMemo(() => pullFromSearch(location.search), [location.search])
  // the last pull request stays rendered while the overlay animates out
  const [shown, setShown] = useState(opened)
  const same = (a: typeof opened, b: typeof opened) => a !== null && b !== null && pullParam(a.ref) === pullParam(b.ref) && a.commentId === b.commentId
  if (opened && !same(opened, shown)) setShown(opened)
  const close = useCallback(() => {
    if ((location.state as { pullOverlay?: boolean } | null)?.pullOverlay) void navigate(-1)
    else void navigate({ pathname: location.pathname, search: withoutPull(location.search) }, { replace: true, state: location.state })
  }, [location, navigate])

  return <Dialog.Root
    open={opened !== null}
    onOpenChange={(open, details) => {
      if (open) return
      if (details.reason === 'escape-key') {
        // Esc first leaves a text field (a comment box closes itself); the next Esc closes the pull request
        if (details.event.defaultPrevented) return
        const field = focused()
        if (field instanceof HTMLElement && field.matches('input, textarea, select, [contenteditable="true"]')) { field.blur(); return }
      }
      close()
    }}
    onOpenChangeComplete={(open) => { if (!open) setShown(null) }}
  >
    <Dialog.Portal>
      <Dialog.Backdrop className="pull-overlay-backdrop fixed inset-0 z-50 bg-black/40 supports-backdrop-filter:backdrop-blur-[2px]" />
      <Dialog.Popup data-pull-overlay aria-label="Pull request" className="pull-overlay fixed inset-2 z-50 flex flex-col overflow-hidden rounded-xl border bg-background text-foreground shadow-2xl outline-none sm:inset-3 dark:shadow-black/60">
        {shown ? <PullReview key={pullParam(shown.ref)} pullRef={shown.ref} commentId={shown.commentId} onClose={close} /> : null}
      </Dialog.Popup>
    </Dialog.Portal>
  </Dialog.Root>
}

/** The old page URL (`/tasks/pulls/o/r/n?comment=id`, from older links and notices) opens the overlay over Tasks. */
export function PullRedirect() {
  const { owner = '', repo = '', number = '' } = useParams()
  const { search } = useLocation()
  const comment = Number(new URLSearchParams(search).get('comment'))
  return <Navigate replace to={`/tasks${withPull('', { owner, repo, number: Number(number) }, comment > 0 ? comment : undefined)}`} />
}
