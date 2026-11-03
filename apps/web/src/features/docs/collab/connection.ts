import { createContext } from 'react'
import type { CollabConnection } from '@orbit/editor/collab/session'
import { collabSocket, COLLAB_FRAGMENT, type CollabTarget } from '@orbit/editor/collab/target'

/** Opens a collaborative document (the socket starts connecting right away). */
export type CollabConnect = (target: CollabTarget) => Promise<CollabConnection>

/**
 * The real connection: `WebsocketProvider` on `/api/v1/workspaces/{ws}/{pages|tasks}/{id}/collab?v=1&epoch=…` (same
 * origin, so the session cookie rides along). yjs + y-websocket load on demand, next to the lazy editor chunk.
 */
export const connectDoc: CollabConnect = async (target) => {
  const [Y, { WebsocketProvider }] = await Promise.all([import('yjs'), import('y-websocket')])
  const doc = new Y.Doc()
  const { serverUrl, room, params } = collabSocket(window.location.origin, target)
  const provider = new WebsocketProvider(serverUrl, room, doc, {
    params,
    // Tabs of the same browser sync through the server like everyone else (and each shows up in presence once).
    disableBc: true,
    maxBackoffTime: 5000,
  })
  return { doc, provider, fragment: doc.getXmlFragment(COLLAB_FRAGMENT) }
}

/** Lets tests (and stories) swap the socket for a fake provider. */
export const CollabConnectContext = createContext<CollabConnect>(connectDoc)
