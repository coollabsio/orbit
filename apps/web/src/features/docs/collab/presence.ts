import { useMemo, useSyncExternalStore } from 'react'
import type { Awareness } from 'y-protocols/awareness'

/** Someone else on the page, as the server stamped them into awareness (`user: { id, name, color }`). */
export interface Collaborator {
  id: string
  name: string
  color: string
}

/**
 * Other people on the page: one entry per user id (several tabs count once), without the local client and without
 * the current user's other tabs. States without a server-stamped id (not yet seen by the server) are skipped.
 * Order: first seen, i.e. by awareness client order, which is stable while people stay.
 */
export function collaboratorsOf(
  states: ReadonlyMap<number, Record<string, unknown>>,
  localClientId: number,
  selfUserId: string | null | undefined,
): Collaborator[] {
  const seen = new Map<string, Collaborator>()
  for (const [clientId, state] of states) {
    if (clientId === localClientId) continue
    const user = state?.user as Partial<Collaborator> | undefined
    if (!user || typeof user.id !== 'string' || !user.id) continue
    if (user.id === selfUserId || seen.has(user.id)) continue
    seen.set(user.id, {
      id: user.id,
      name: typeof user.name === 'string' && user.name.trim() ? user.name : 'Someone',
      color: typeof user.color === 'string' ? user.color : '#737373',
    })
  }
  return [...seen.values()]
}

function sameList(a: Collaborator[], b: Collaborator[]): boolean {
  return a.length === b.length && a.every((item, index) => item.id === b[index].id && item.name === b[index].name && item.color === b[index].color)
}

const NOBODY: Collaborator[] = []

/** An external store over awareness whose snapshot only changes when the list does (not on every cursor move). */
function collaboratorStore(awareness: Awareness | null, selfUserId: string | null | undefined) {
  let current = NOBODY
  return {
    subscribe(notify: () => void) {
      if (!awareness) return () => {}
      awareness.on('change', notify)
      return () => awareness.off('change', notify)
    },
    snapshot(): Collaborator[] {
      if (!awareness) return NOBODY
      const next = collaboratorsOf(awareness.getStates() as Map<number, Record<string, unknown>>, awareness.clientID, selfUserId)
      if (!sameList(current, next)) current = next
      return current
    },
  }
}

/** Live list of other people on the page. */
export function useCollaborators(awareness: Awareness | null, selfUserId: string | null | undefined): Collaborator[] {
  const store = useMemo(() => collaboratorStore(awareness, selfUserId), [awareness, selfUserId])
  return useSyncExternalStore(store.subscribe, store.snapshot)
}
