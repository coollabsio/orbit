import { useSyncExternalStore } from 'react'

/** Drafts live in `localStorage` only (draft sync is a backend project); one JSON map for each workspace. */
type DraftMap = Record<string, string>

const draftsKey = (workspaceId: string) => `orbit:chat:drafts:${workspaceId}`
const lastOpenedKey = (workspaceId: string) => `orbit:chat:last-opened:${workspaceId}`
const entryKey = (conversationId: string, threadRootId?: string | null) =>
  threadRootId ? `${conversationId}:${threadRootId}` : conversationId

const listeners = new Set<() => void>()
/** Parsed maps, so reads during render do not parse JSON and snapshots stay stable. */
const cache = new Map<string, DraftMap>()

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null
  } catch {
    return null
  }
}

function read(workspaceId: string): DraftMap {
  const cached = cache.get(workspaceId)
  if (cached) return cached
  let drafts: DraftMap = {}
  try {
    const parsed: unknown = JSON.parse(storage()?.getItem(draftsKey(workspaceId)) ?? '{}')
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) drafts = parsed as DraftMap
  } catch {
    // Unreadable storage or JSON: start without drafts.
  }
  cache.set(workspaceId, drafts)
  return drafts
}

function write(workspaceId: string, drafts: DraftMap) {
  cache.set(workspaceId, drafts)
  try {
    if (Object.keys(drafts).length === 0) storage()?.removeItem(draftsKey(workspaceId))
    else storage()?.setItem(draftsKey(workspaceId), JSON.stringify(drafts))
  } catch {
    // Storage is full or blocked: the draft still lives in memory for this page.
  }
  for (const listener of listeners) listener()
}

export function getDraft(workspaceId: string, conversationId: string, threadRootId?: string | null): string {
  return read(workspaceId)[entryKey(conversationId, threadRootId)] ?? ''
}

/** Saves the draft; text with nothing but whitespace removes it. */
export function setDraft(workspaceId: string, conversationId: string, threadRootId: string | null | undefined, text: string) {
  const key = entryKey(conversationId, threadRootId)
  const current = read(workspaceId)
  if (text.trim() === '') {
    if (!(key in current)) return
    const { [key]: _removed, ...rest } = current
    write(workspaceId, rest)
    return
  }
  if (current[key] === text) return
  write(workspaceId, { ...current, [key]: text })
}

export function clearDraft(workspaceId: string, conversationId: string, threadRootId?: string | null) {
  setDraft(workspaceId, conversationId, threadRootId, '')
}

export function subscribeDrafts(listener: () => void): () => void {
  const onStorage = (event: StorageEvent) => {
    // Another tab changed a draft: drop the parsed copy.
    if (event.key !== null && !event.key.startsWith('orbit:chat:drafts:')) return
    cache.clear()
    listener()
  }
  listeners.add(listener)
  globalThis.addEventListener?.('storage', onStorage)
  return () => {
    listeners.delete(listener)
    globalThis.removeEventListener?.('storage', onStorage)
  }
}

/** The conversation's own composer has a draft (thread drafts do not count): the sidebar row shows a pencil. */
export function useHasDraft(workspaceId: string, conversationId: string): boolean {
  return useSyncExternalStore(subscribeDrafts, () => getDraft(workspaceId, conversationId) !== '')
}

/** Live draft text, for a composer that is not the one being typed in (pane and full view share a draft). */
export function useDraft(workspaceId: string, conversationId: string, threadRootId?: string | null): string {
  return useSyncExternalStore(subscribeDrafts, () => getDraft(workspaceId, conversationId, threadRootId))
}

export function getLastOpenedConversation(workspaceId: string): string | null {
  try {
    return storage()?.getItem(lastOpenedKey(workspaceId)) ?? null
  } catch {
    return null
  }
}

export function setLastOpenedConversation(workspaceId: string, conversationId: string | null) {
  try {
    if (conversationId) storage()?.setItem(lastOpenedKey(workspaceId), conversationId)
    else storage()?.removeItem(lastOpenedKey(workspaceId))
  } catch {
    // Not being able to remember the last conversation is harmless.
  }
}
