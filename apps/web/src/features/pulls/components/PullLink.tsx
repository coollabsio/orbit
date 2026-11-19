import type { PullRef } from '@orbit/api/github'
import { Link, useLocation, useNavigate, type LinkProps } from 'react-router'
import { withPull } from '@/features/pulls/lib/overlay'

/** Where a pull request opens: over the current page, with its own history entry, so Back closes it. */
function usePullTarget() {
  const location = useLocation()
  return (ref: PullRef, commentId?: number) => ({
    to: { pathname: location.pathname, search: withPull(location.search, ref, commentId) },
    // the page's own state stays (a task opened from the list still closes back to it)
    state: { ...(location.state as object | null), pullOverlay: true },
  })
}

/** A link that opens a pull request over the page it is on. */
export function PullLink({ pullRef, commentId, ...props }: Omit<LinkProps, 'to' | 'state'> & { pullRef: PullRef; commentId?: number }) {
  const target = usePullTarget()(pullRef, commentId)
  return <Link {...props} to={target.to} state={target.state} />
}

/** Opens a pull request over the current page, from code (an inbox notice). */
export function useOpenPull() {
  const navigate = useNavigate()
  const target = usePullTarget()
  return (ref: PullRef, commentId?: number) => {
    const { to, state } = target(ref, commentId)
    void navigate(to, { state })
  }
}
