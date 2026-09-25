import { useContext, useEffect, useRef, useState } from 'react'
import { CollabConnectContext, type CollabTarget } from './connection'
import { CollabSession, type CollabConnection, type CollabProblem, type CollabState } from './session'

export interface CollabUser {
  name: string
  color: string
}

export interface UseCollabSessionOptions extends CollabTarget {
  /** Bump to drop the local document and open a fresh session (after a 4409 reset). */
  generation: number
  /** Local awareness user (the server overwrites it with the session's user for everyone else). */
  user: CollabUser
  onProblem: (problem: CollabProblem) => void
  onProbe: () => void
}

export interface CollabSessionView {
  /** Changes whenever a new document/provider is opened: key the editor with it. */
  key: string
  /** Set once the socket was created; the editor may mount only when `state.ready`. */
  connection: CollabConnection | null
  state: CollabState
  /** Live session object (for `hasUnsyncedChanges` and `fail`); null until connected. */
  session: CollabSession | null
}

const INITIAL: CollabState = { status: 'connecting', problem: null, ready: false }

/**
 * One Y.Doc + provider per page, epoch and generation: created on mount, destroyed on page switch and unmount.
 * Never bind an editor before `state.ready` (the first sync): an unsynced fragment is empty.
 */
export function useCollabSession({ workspaceId, pageId, epoch, generation, user, onProblem, onProbe }: UseCollabSessionOptions): CollabSessionView {
  const connect = useContext(CollabConnectContext)
  const key = `${pageId}:${epoch}:${generation}`
  const [view, setView] = useState<Omit<CollabSessionView, 'key'> & { key: string | null }>({
    key: null,
    connection: null,
    state: INITIAL,
    session: null,
  })
  const handlers = useRef({ onProblem, onProbe, user })
  useEffect(() => {
    handlers.current = { onProblem, onProbe, user }
  })

  useEffect(() => {
    let cancelled = false
    let session: CollabSession | null = null
    void connect({ workspaceId, pageId, epoch }).then(
      (connection) => {
        if (cancelled) {
          connection.provider.destroy()
          connection.doc.destroy()
          return
        }
        // Presence works before the editor mounts; BlockNote sets the same field again when it binds.
        connection.provider.awareness.setLocalStateField('user', handlers.current.user)
        session = new CollabSession(connection, {
          onChange: (state) => {
            if (!cancelled) setView((current) => (current.session === session ? { ...current, state } : current))
          },
          onProblem: (problem) => {
            if (!cancelled) handlers.current.onProblem(problem)
          },
          onProbe: () => {
            if (!cancelled) handlers.current.onProbe()
          },
        })
        setView({ key, connection, state: session.state, session })
      },
      () => {
        // Loading the Yjs chunk failed (offline right at page open): the page stays "Connecting…".
      },
    )
    return () => {
      cancelled = true
      session?.destroy()
      setView({ key: null, connection: null, state: INITIAL, session: null })
    }
  }, [connect, workspaceId, pageId, epoch, generation, key])

  // A stale view (previous page/epoch) never reaches the caller.
  if (view.key !== key) return { key, connection: null, state: INITIAL, session: null }
  return { ...view, key }
}
