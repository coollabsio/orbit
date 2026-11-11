import { useQueryClient } from '@tanstack/react-query'
import { useEffect } from 'react'
import { WifiOff } from 'reicon-react'
import { queryKeys } from '@/api/queryKeys'
import { useCurrentUser, useSetupStatus } from '@/features/auth/api'
import { useWorkspaces } from '@/features/workspaces/api'
import { onConnectionRetry, queryLostConnection, reportConnection, useConnection } from '@/lib/connection'

/** The queries the gates above the shell render from. */
const CORE_QUERY_KEYS = [queryKeys.setup, queryKeys.currentUser, queryKeys.workspaces]

/**
 * Tells the shared connection status when a core query cannot reach the server, and fetches the failed ones again
 * when the user presses Retry or the events socket is back (`live`). They retry by themselves only with backoff, and
 * give up after about a minute: no timer here asks again.
 */
function useCoreQueryConnection(live: boolean) {
  const queryClient = useQueryClient()
  const setup = useSetupStatus()
  const user = useCurrentUser()
  const workspaces = useWorkspaces()
  const lost = queryLostConnection(setup) || queryLostConnection(user) || queryLostConnection(workspaces)

  useEffect(() => {
    reportConnection('queries', lost)
    return () => reportConnection('queries', false)
  }, [lost])

  useEffect(() => {
    const refetchFailed = () => {
      for (const queryKey of CORE_QUERY_KEYS) {
        void queryClient.refetchQueries({ queryKey, exact: true, predicate: (query) => query.state.status === 'error' })
      }
    }
    // The server answers again: a query that gave up meanwhile has no reason to stay failed.
    if (live) refetchFailed()
    return onConnectionRetry(refetchFailed)
  }, [live, queryClient])
}

/**
 * A small pill that floats over the top of the main pane for the first minute the app cannot reach the server. It does
 * not take layout space, so the page does not move. After that `ConnectionScreen` covers the app.
 */
export function ConnectionBanner({ live }: { live: boolean }) {
  useCoreQueryConnection(live)
  const { phase } = useConnection()
  if (phase !== 'lost') return null
  return (
    <div
      data-slot="connection-banner"
      role="status"
      className="pointer-events-none absolute top-2 left-1/2 z-30 flex max-w-[calc(100%-1rem)] -translate-x-1/2 items-center gap-1.5 rounded-full border border-border bg-popover px-2.5 py-0.5 text-[0.6875rem] text-muted-foreground shadow-sm duration-200 animate-in fade-in"
    >
      <WifiOff className="size-3 shrink-0" />
      <span className="min-w-0 truncate">Reconnecting…</span>
    </div>
  )
}
