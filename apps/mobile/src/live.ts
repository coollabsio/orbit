import type { ChatEvent } from '@orbit/api/generated/types.gen'
import { useQueryClient } from '@tanstack/react-query'
import { useEffect } from 'react'
import { AppState, Platform } from 'react-native'

import { keys } from './data/keys'
import { chatResyncKeys, LiveCursor, parseLiveFrame, parseWorkspaceEvent, socketUrl, workspaceEventKeys } from './data/live'
import { reconnectingSocket, type SocketLike } from './data/socket'
import { applyChatEvents } from './chat-cache'
import { useOrbit } from './orbit-context'


/** React Native's `WebSocket` takes the upgrade headers as a third argument. The DOM typings in scope do not know it. */
const NativeWebSocket = WebSocket as unknown as new (url: string, protocols: null, options: { headers: Record<string, string> }) => SocketLike

const isChatEvent = (value: unknown): value is ChatEvent => typeof value === 'object' && value !== null && typeof (value as { type?: unknown }).type === 'string'

/**
 * Keeps the workspace's queries fresh while the app is open: the workspace events socket marks task, project and inbox
 * queries stale, and the live socket patches chat in place. Both send the session as a bearer token in the upgrade.
 * In the background both close; back in the foreground they connect at once and resume where they stopped.
 */
export function useLiveUpdates() {
  const { session, workspaceId } = useOrbit()
  const queryClient = useQueryClient()

  useEffect(() => {
    // A browser WebSocket cannot send the bearer header, so the web preview runs without live updates.
    if (Platform.OS === 'web') return
    const { server, token } = session
    const create = (url: string) => new NativeWebSocket(url, null, { headers: { Authorization: `Bearer ${token}` } })
    const invalidate = (queryKeys: readonly (readonly string[])[]) => {
      for (const queryKey of queryKeys) void queryClient.invalidateQueries({ queryKey })
    }
    // A closed session: the next request gets the 401 that signs out.
    const ended = () => invalidate([keys.workspaces])

    let after: string | undefined
    const events = reconnectingSocket({
      url: () => socketUrl(server, `/api/v1/workspaces/${workspaceId}/events`, { after }),
      create,
      onEnded: ended,
      onMessage: (raw) => {
        const event = parseWorkspaceEvent(raw)
        if (!event) return
        after = event.sequence
        invalidate(workspaceEventKeys(event, workspaceId))
      },
    })

    const cursor = new LiveCursor()
    const live = reconnectingSocket({
      url: () => socketUrl(server, `/api/v1/workspaces/${workspaceId}/live`, cursor.query()),
      create,
      onEnded: ended,
      silenceMs: 45_000,
      onMessage: (raw) => {
        const frame = parseLiveFrame(raw)
        if (!frame) return
        cursor.read(frame)
        if (frame.type === 'resync') invalidate(chatResyncKeys(workspaceId))
        if (frame.type === 'event' && frame.topic === 'chat' && isChatEvent(frame.event)) {
          applyChatEvents(queryClient, workspaceId, [frame.event])
        }
      },
    })

    const appState = AppState.addEventListener('change', (next) => {
      if (next === 'active') {
        events.reconnect()
        live.reconnect()
      } else if (next === 'background') {
        events.close()
        live.close()
      }
    })
    return () => {
      appState.remove()
      events.close()
      live.close()
    }
  }, [queryClient, session, workspaceId])
}
