import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { queryKeys } from '../../api/queryKeys'
import { parseEvent } from './events'

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
    let refreshing = false
    setConnected(false)

    const refresh = async () => {
      // Remote edits must not remount a focused draft or race optimistic writes.
      if (disposed || pending === undefined || refreshing || client.isMutating() > 0) return
      const active = document.activeElement
      if (active instanceof HTMLElement && active.matches('input, textarea, [contenteditable="true"]')) return
      refreshing = true
      const next = pending
      pending = undefined
      try {
        await client.invalidateQueries({ queryKey: queryKeys.workspace(workspaceId) }, { throwOnError: true })
        cursor = next
      } catch {
        pending ??= next
      } finally { refreshing = false }
    }
    const connect = () => {
      if (disposed) return
      const url = new URL(`/api/v1/workspaces/${workspaceId}/events`, window.location.href)
      url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
      if (cursor !== undefined) url.searchParams.set('after', cursor)
      socket = new WebSocket(url)
      socket.onopen = () => { setConnected(true); delay = 1000 }
      socket.onmessage = (message) => {
        const event = parseEvent(message.data)
        if (!event) { socket?.close(); return }
        pending = event.sequence
        void refresh()
      }
      socket.onclose = () => {
        if (disposed) return
        setConnected(false)
        // Revalidate revoked sessions and removed memberships through HTTP.
        void client.invalidateQueries({ queryKey: queryKeys.currentUser })
        void client.invalidateQueries({ queryKey: queryKeys.workspaces })
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
