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
 * One line above the top bar for the first minute the app cannot reach the server; the pages stay on screen below it.
 * After that `ConnectionScreen` covers the app.
 */
export function ConnectionBanner({ live }: { live: boolean }) {
  useCoreQueryConnection(live)
  const { phase } = useConnection()
  if (phase !== 'lost') return null
  return (
    <div
      data-slot="connection-banner"
      role="status"
      className="flex min-h-8 shrink-0 items-center justify-center gap-2 border-b border-border bg-muted px-3 py-1 text-xs text-muted-foreground duration-200 animate-in fade-in"
    >
      <WifiOff className="size-3.5 shrink-0" />
      <span className="min-w-0 truncate">Orbit lost connection, please wait</span>
    </div>
  )
}
