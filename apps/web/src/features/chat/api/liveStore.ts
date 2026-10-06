import { useSyncExternalStore } from 'react'
import { reportConnection } from '@/lib/connection'
import type { ConnectionStatus } from './types'

/**
 * Typing and connection status change often and are never fetched, so they stay out of the query cache.
 * One set of stores for the page: `ChatProvider` resets them when the workspace changes. Presence is app-wide and lives
 * in `features/realtime/presence.ts`.
 */
const TYPING_TTL = 10_000
const NOBODY: readonly string[] = []

const listeners = new Set<() => void>()
const typingTimers = new Map<string, Map<string, ReturnType<typeof setTimeout>>>()
/** Immutable snapshots: a new array or set for each change, so `useSyncExternalStore` sees it. */
const typingSnapshots = new Map<string, readonly string[]>()
let connection: ConnectionStatus = 'connected'

const typingKey = (conversationId: string, threadRootId?: string | null) => `${conversationId}:${threadRootId ?? ''}`

function emit() {
  for (const listener of listeners) listener()
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

function snapshotTyping(key: string) {
  const users = typingTimers.get(key)
  if (users && users.size > 0) typingSnapshots.set(key, [...users.keys()])
  else typingSnapshots.delete(key)
  emit()
}

/** A member is typing; forgotten after 10 seconds unless they signal again. */
export function noteTyping(conversationId: string, threadRootId: string | null, userId: string) {
  const key = typingKey(conversationId, threadRootId)
  const users = typingTimers.get(key) ?? new Map<string, ReturnType<typeof setTimeout>>()
  typingTimers.set(key, users)
  const known = users.has(userId)
  clearTimeout(users.get(userId))
  users.set(
    userId,
    setTimeout(() => clearTyping(conversationId, threadRootId, userId), TYPING_TTL),
  )
  if (!known) snapshotTyping(key)
}

/** Their message arrived (or the signal expired). */
export function clearTyping(conversationId: string, threadRootId: string | null, userId: string) {
  const key = typingKey(conversationId, threadRootId)
  const users = typingTimers.get(key)
  if (!users?.has(userId)) return
  clearTimeout(users.get(userId))
  users.delete(userId)
  snapshotTyping(key)
}

export function setConnectionStatus(status: ConnectionStatus) {
  if (connection === status) return
  connection = status
  // The banner in the app shell reads the shared status; `ConnectionLine` in the composer reads this store. `ended` is
  // not a lost connection: the banner would say "please wait" for a retry that never comes.
  reportConnection('chat', status === 'reconnecting')
  emit()
}

export function resetLiveStore() {
  for (const users of typingTimers.values()) for (const timer of users.values()) clearTimeout(timer)
  typingTimers.clear()
  typingSnapshots.clear()
  connection = 'connected'
  reportConnection('chat', false)
  emit()
}

/** Ids of the members typing in a conversation's composer, or in one of its threads. */
export function useTyping(conversationId: string, threadRootId?: string | null): readonly string[] {
  return useSyncExternalStore(subscribe, () => typingSnapshots.get(typingKey(conversationId, threadRootId)) ?? NOBODY)
}

export function useConnectionStatus(): ConnectionStatus {
  return useSyncExternalStore(subscribe, () => connection)
}
