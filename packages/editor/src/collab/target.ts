/** The Y.Doc fragment BlockNote binds to; the server's converter reads and writes the same name. */
export const COLLAB_FRAGMENT = 'prosemirror'
/** `?v=` of the collab socket (`PROTOCOL_VERSION` in `apps/server/src/collab/mod.rs`). */
export const COLLAB_PROTOCOL_VERSION = '1'

/** The REST collection of a collaborative document: a docs page or a task (its description). */
export type CollabKind = 'pages' | 'tasks'

export interface CollabTarget {
  workspaceId: string
  kind: CollabKind
  id: string
  /** `Page.collab_epoch` / `Task.collab_epoch`; a stale one is closed with 4409. */
  epoch: string
}

/**
 * y-websocket's server URL and room for a document: the socket opens `{server}/{room}?v=1&epoch=…` on an `http(s)`
 * origin turned `ws(s)`.
 */
export function collabSocket(origin: string, { workspaceId, kind, id, epoch }: CollabTarget) {
  return {
    serverUrl: `${origin.replace(/^http/, 'ws').replace(/\/+$/, '')}/api/v1/workspaces/${workspaceId}/${kind}/${id}`,
    room: 'collab',
    params: { v: COLLAB_PROTOCOL_VERSION, epoch } as Record<string, string>,
  }
}
