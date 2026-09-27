// "Edited <time> by <name>" for the page header: the server's last editor (`updated_by_user`, `updated_at`) and, while
// the page is open, edits seen live on the collaborative document (attributed through the awareness user stamps).
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { Page } from '@/api/generated/types.gen'
import type { CollabConnection } from './collab/session'

export interface LastEdit {
  userId: string
  name: string
  /** Epoch milliseconds. */
  at: number
}

const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR

function plural(count: number, unit: string): string {
  return `${count} ${unit}${count === 1 ? '' : 's'} ago`
}

/** "just now", "5 minutes ago", "2 hours ago", "3 days ago", or "on Sep 5" for older edits. */
export function relativeEditTime(at: number, now: number): string {
  const diff = Math.max(0, now - at)
  if (diff < MINUTE) return 'just now'
  if (diff < HOUR) return plural(Math.floor(diff / MINUTE), 'minute')
  if (diff < DAY) return plural(Math.floor(diff / HOUR), 'hour')
  if (diff < 7 * DAY) return plural(Math.floor(diff / DAY), 'day')
  return `on ${new Date(at).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}`
}

export function lastEditedLabel(edit: LastEdit, selfId: string | null, now: number): string {
  const who = edit.userId === selfId ? 'you' : edit.name.trim() || 'someone'
  return `Edited ${relativeEditTime(edit.at, now)} by ${who}`
}

export function serverLastEdit(page: Page): LastEdit {
  return { userId: page.updated_by_user.id, name: page.updated_by_user.display_name, at: Date.parse(page.updated_at) }
}

interface UpdateTransaction {
  beforeState: Map<number, number>
  afterState: Map<number, number>
}

/** Client ids whose state advanced in a Yjs transaction (the authors of the change). */
export function authorsOf(transaction: UpdateTransaction): number[] {
  const authors: number[] = []
  for (const [client, clock] of transaction.afterState) {
    if (clock > (transaction.beforeState.get(client) ?? 0)) authors.push(client)
  }
  return authors
}

/**
 * The newest known edit of the page: the server's, or one seen live on the open document (local typing → the current
 * user; remote updates → the awareness user of the client that wrote them). Re-renders every 30 s so the relative time
 * stays current.
 */
export function useLastEdited(
  page: Page,
  connection: CollabConnection | null,
  ready: boolean,
  self: { id: string; name: string } | null,
): { edit: LastEdit; now: number } {
  const [live, setLive] = useState<LastEdit | null>(null)
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000)
    return () => clearInterval(timer)
  }, [])

  const selfId = self?.id ?? null
  const selfName = self?.name ?? ''
  // Read when an update arrives, not as an effect dependency: re-subscribing after `ready` flips left a gap in which a
  // collaborator's first edit was missed. A layout effect updates it in the same commit that sets `ready` (before the
  // editor mounts); a passive effect ran after the editor was already editable.
  const readyRef = useRef(ready)
  useLayoutEffect(() => {
    readyRef.current = ready
  }, [ready])
  useEffect(() => {
    if (!connection) return
    const { doc, provider } = connection
    // Keystrokes arrive many times a second; the label only needs about minute precision.
    const record = (userId: string, name: string, at: number) =>
      setLive((previous) => (previous && previous.userId === userId && at - previous.at < 15_000 ? previous : { userId, name, at }))
    const onUpdate = (_update: Uint8Array, origin: unknown, _doc: unknown, transaction: UpdateTransaction) => {
      // Only after the first sync: the initial state transfer is not an edit.
      if (!readyRef.current) return
      const authors = authorsOf(transaction)
      if (authors.length === 0) return
      const at = Date.now()
      if (origin !== provider) {
        // Typed here (the editor, undo): our own edit.
        if (selfId && authors.includes(doc.clientID)) record(selfId, selfName, at)
        return
      }
      const states = provider.awareness.getStates() as Map<number, { user?: { id?: unknown; name?: unknown } }>
      for (const client of authors) {
        const user = states.get(client)?.user
        if (user && typeof user.id === 'string') {
          record(user.id, typeof user.name === 'string' ? user.name : '', at)
          return
        }
      }
    }
    doc.on('update', onUpdate)
    return () => doc.off('update', onUpdate)
  }, [connection, selfId, selfName])

  const server = serverLastEdit(page)
  const edit = live && live.at > server.at ? live : server
  return { edit, now: Math.max(now, edit.at) }
}
