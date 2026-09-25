import { createContext } from 'react'
import type { CollabConnection } from './session'

/** The Y.Doc fragment BlockNote binds to; the server's converter reads and writes the same name. */
export const COLLAB_FRAGMENT = 'prosemirror'
/** `?v=` of the collab socket (`PROTOCOL_VERSION` in `apps/server/src/collab/mod.rs`). */
export const COLLAB_PROTOCOL_VERSION = '1'

export interface CollabTarget {
  workspaceId: string
  pageId: string
  /** `Page.collab_epoch`; a stale one is closed with 4409. */
  epoch: string
}

/** Opens a page's collaborative document (the socket starts connecting right away). */
export type CollabConnect = (target: CollabTarget) => Promise<CollabConnection>

/**
 * The real connection: `WebsocketProvider` on `/api/v1/workspaces/{ws}/pages/{id}/collab?v=1&epoch=…` (same origin,
 * so the session cookie rides along). yjs + y-websocket load on demand, next to the lazy editor chunk.
 */
export const connectPage: CollabConnect = async ({ workspaceId, pageId, epoch }) => {
  const [Y, { WebsocketProvider }] = await Promise.all([import('yjs'), import('y-websocket')])
  const doc = new Y.Doc()
  const origin = window.location.origin.replace(/^http/, 'ws')
  const provider = new WebsocketProvider(`${origin}/api/v1/workspaces/${workspaceId}/pages/${pageId}`, 'collab', doc, {
    params: { v: COLLAB_PROTOCOL_VERSION, epoch },
    // Tabs of the same browser sync through the server like everyone else (and each shows up in presence once).
    disableBc: true,
    maxBackoffTime: 5000,
  })
  return { doc, provider, fragment: doc.getXmlFragment(COLLAB_FRAGMENT) }
}

/** Lets tests (and stories) swap the socket for a fake provider. */
export const CollabConnectContext = createContext<CollabConnect>(connectPage)
