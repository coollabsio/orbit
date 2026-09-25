import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { queryKeys } from '@/api/queryKeys'
import { parseEvent } from './events'

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
    let cursor: string | undefined
    let pending: string | undefined
    let pendingWorkspaces = false
    let pendingProfile = false
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
      pending = undefined
      pendingWorkspaces = false
      pendingProfile = false
      try {
        await client.invalidateQueries({ queryKey: queryKeys.workspace(workspaceId) }, { throwOnError: true })
        if (refreshWorkspaces) {
          await client.invalidateQueries({ queryKey: queryKeys.workspaces }, { throwOnError: true })
        }
        if (refreshProfile) {
          await client.invalidateQueries({ queryKey: queryKeys.currentUser }, { throwOnError: true })
        }
        cursor = next
        refreshDelay = 1000
      } catch {
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
      socket = new WebSocket(url)
      let opened = false
      socket.onopen = () => { opened = true; setConnected(true); delay = 1000 }
      socket.onmessage = (message) => {
        const event = parseEvent(message.data)
        if (!event) { socket?.close(); return }
        pending = event.sequence
        pendingWorkspaces ||= event.kind === 'resync_required' || event.workspaces_changed === true
        pendingProfile ||= event.kind === 'resync_required' || event.profile_changed === true
        void refresh()
      }
      socket.onclose = () => {
        if (disposed) return
        setConnected(false)
        // A failed handshake gives no evidence of revocation. Do not turn each retry
        // during an outage or rate limit into two more HTTP requests.
        if (opened) {
          // Revalidate revoked sessions and removed memberships through HTTP.
          void client.invalidateQueries({ queryKey: queryKeys.currentUser })
          void client.invalidateQueries({ queryKey: queryKeys.workspaces })
        }
        retry = setTimeout(connect, delay + Math.random() * 500)
        delay = Math.min(delay * 2, 30000)
      }
    }
    const reconnect = () => { socket?.close() }
    window.addEventListener('online', reconnect)
    const flush = setInterval(() => { void refresh() }, 250)
    connect()
    return () => {
      disposed = true
      window.removeEventListener('online', reconnect)
      clearInterval(flush)
      clearTimeout(retry)
      socket?.close()
    }
  }, [client, workspaceId])
  return connected
}
