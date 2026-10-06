import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { queryKeys } from '@/api/queryKeys'
import { reportConnection, watchReconnect } from '@/lib/connection'
import { parseEvent } from './events'

const VIEW_PREFERENCES = queryKeys.viewPreference('', '')[2]

/** Marks a surface that merges remote updates into its own focused fields safely (e.g. the Docs page editor). */
export const REALTIME_SAFE_ATTRIBUTE = 'data-realtime-safe'

/**
 * A focused input/textarea/contenteditable pauses refreshes so a remote edit cannot remount a draft. Surfaces marked
 * with `data-realtime-safe` opt out: they keep local edits themselves, and they are focused almost all the time, so
 * pausing there would stop every workspace refresh (Docs titles, tree, favorites) while someone types.
 */
export function focusedDraftBlocksRefresh(active: Element | null = document.activeElement): boolean {
  if (!(active instanceof HTMLElement) || !active.matches('input, textarea, [contenteditable="true"]')) return false
  return active.closest(`[${REALTIME_SAFE_ATTRIBUTE}]`) === null
}

export function useWorkspaceEvents(workspaceId: string) {
  const client = useQueryClient()
  const [connected, setConnected] = useState(false)
  useEffect(() => {
    let disposed = false
    let socket: WebSocket | undefined
    let retry: ReturnType<typeof setTimeout> | undefined
    let delay = 1000
    /** The socket was open once: from then on a closed socket is a lost connection, not a first load. */
    let wasOpen = false
    /** What a lost socket leaves behind, without the retry; it also takes the handlers off that socket. */
    let drop = () => {}
    let cursor: string | undefined
    let pending: string | undefined
    let pendingWorkspaces = false
    let pendingProfile = false
    /** Every pending event only changed saved views (meaningful while `pending` is set). */
    let pendingViewsOnly = true
    let refreshing = false
    let nextRefresh = 0
    let refreshDelay = 1000
    setConnected(false)

    const refresh = async () => {
      // Remote edits must not remount a focused draft or race optimistic writes.
      if (disposed || pending === undefined || refreshing || Date.now() < nextRefresh || client.isMutating() > 0) return
      if (focusedDraftBlocksRefresh()) return
      refreshing = true
      const next = pending
      const refreshWorkspaces = pendingWorkspaces
      const refreshProfile = pendingProfile
      const viewsOnly = pendingViewsOnly
      pending = undefined
      pendingWorkspaces = false
      pendingProfile = false
      pendingViewsOnly = true
      try {
        await client.invalidateQueries(viewsOnly
          // the views list key prefixes every view detail key; tasks and preferences stay untouched
          ? { queryKey: queryKeys.views(workspaceId) }
          : {
              queryKey: queryKeys.workspace(workspaceId),
              // preference writes are never broadcast; a refetch here could land the old value over a debounced edit
              predicate: (query) => query.queryKey[2] !== VIEW_PREFERENCES,
            }, { throwOnError: true })
        if (refreshWorkspaces) {
          await client.invalidateQueries({ queryKey: queryKeys.workspaces }, { throwOnError: true })
        }
        if (refreshProfile) {
          await client.invalidateQueries({ queryKey: queryKeys.currentUser }, { throwOnError: true })
        }
        cursor = next
        refreshDelay = 1000
      } catch {
        pendingViewsOnly = pending === undefined ? viewsOnly : pendingViewsOnly && viewsOnly
        pending ??= next
        pendingWorkspaces ||= refreshWorkspaces
        pendingProfile ||= refreshProfile
        nextRefresh = Date.now() + refreshDelay
        refreshDelay = Math.min(refreshDelay * 2, 30000)
      } finally { refreshing = false }
    }
    const connect = () => {
      if (disposed) return
      const url = new URL(`/api/v1/workspaces/${workspaceId}/events`, window.location.href)
      url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
      if (cursor !== undefined) url.searchParams.set('after', cursor)
      retry = undefined
      socket = new WebSocket(url)
      let opened = false
      socket.onopen = () => { opened = true; wasOpen = true; setConnected(true); reportConnection('events', false); delay = 1000 }
      socket.onmessage = (message) => {
        const event = parseEvent(message.data)
        if (!event) { socket?.close(); return }
        pendingViewsOnly = (pending === undefined || pendingViewsOnly) && event.kind !== 'resync_required' && event.views_only === true
        pending = event.sequence
        pendingWorkspaces ||= event.kind === 'resync_required' || event.workspaces_changed === true
        pendingProfile ||= event.kind === 'resync_required' || event.profile_changed === true
        void refresh()
      }
      const current = socket
      drop = () => {
        current.onopen = current.onmessage = current.onclose = null
        setConnected(false)
        if (wasOpen) reportConnection('events', true)
        // A failed handshake gives no evidence of revocation. Do not turn each retry
        // during an outage or rate limit into two more HTTP requests.
        if (opened) {
          // Revalidate revoked sessions and removed memberships through HTTP.
          void client.invalidateQueries({ queryKey: queryKeys.currentUser })
          void client.invalidateQueries({ queryKey: queryKeys.workspaces })
        }
      }
      socket.onclose = () => {
        if (disposed) return
        drop()
        retry = setTimeout(connect, delay + Math.random() * 500)
        delay = Math.min(delay * 2, 30000)
      }
    }
    /** Back online, the tab is visible again, or the user pressed Retry: skip the rest of the backoff wait. */
    const reconnectNow = () => {
      if (disposed || retry === undefined) return
      clearTimeout(retry)
      connect()
    }
    // An open socket may be dead after the network changed: drop it and start the next one at once. Not through
    // `onclose`: the browser reports the close only later, and then the backoff wait would follow.
    const reconnect = () => {
      if (disposed || retry !== undefined || !socket) return
      const current = socket
      drop()
      current.close()
      connect()
    }
    window.addEventListener('online', reconnect)
    // The tab is visible again or Retry: only a socket that waits for its retry starts again.
    const stopWatching = watchReconnect(reconnectNow)
    const flush = setInterval(() => { void refresh() }, 250)
    connect()
    return () => {
      disposed = true
      window.removeEventListener('online', reconnect)
      stopWatching()
      reportConnection('events', false)
      clearInterval(flush)
      clearTimeout(retry)
      socket?.close()
    }
  }, [client, workspaceId])
  return connected
}
