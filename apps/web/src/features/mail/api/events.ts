import type { QueryClient } from '@tanstack/react-query'
import { mailKeys } from './keys'

/** What the `mail` topic of the live socket sends: a thread of a mailbox the user reads changed or arrived. */
export interface MailLiveEvent {
  type: 'thread.updated'
  mailbox_id: string
  thread_id: string
}

export function parseMailEvent(value: unknown): MailLiveEvent | null {
  if (!value || typeof value !== 'object') return null
  const event = value as Record<string, unknown>
  if (event.type !== 'thread.updated' || typeof event.mailbox_id !== 'string' || typeof event.thread_id !== 'string') return null
  return { type: event.type, mailbox_id: event.mailbox_id, thread_id: event.thread_id }
}

/** The first wait after a failed refresh; it doubles up to `MAX_RETRY_MS` (`.ai/lessons.md`, API retry load). */
const FIRST_RETRY_MS = 1000
const MAX_RETRY_MS = 30_000
/** While a mail write is in flight its own invalidation follows; a refresh now could land over its optimistic patch. */
const MUTATING_WAIT_MS = 300

interface Refresh {
  threads: Set<string>
  running: boolean
  timer: ReturnType<typeof setTimeout> | undefined
  delay: number
}

const refreshes = new WeakMap<QueryClient, Map<string, Refresh>>()

function refreshFor(queryClient: QueryClient, workspaceId: string): Refresh {
  let byWorkspace = refreshes.get(queryClient)
  if (!byWorkspace) refreshes.set(queryClient, (byWorkspace = new Map()))
  let refresh = byWorkspace.get(workspaceId)
  if (!refresh) byWorkspace.set(workspaceId, (refresh = { threads: new Set(), running: false, timer: undefined, delay: FIRST_RETRY_MS }))
  return refresh
}

function later(queryClient: QueryClient, workspaceId: string, refresh: Refresh, ms: number) {
  refresh.timer = setTimeout(() => {
    refresh.timer = undefined
    void run(queryClient, workspaceId, refresh)
  }, ms)
}

async function run(queryClient: QueryClient, workspaceId: string, refresh: Refresh) {
  if (refresh.running || refresh.timer !== undefined || refresh.threads.size === 0) return
  if (queryClient.isMutating({ mutationKey: mailKeys.all(workspaceId) }) > 0) {
    later(queryClient, workspaceId, refresh, MUTATING_WAIT_MS)
    return
  }
  refresh.running = true
  const threads = [...refresh.threads]
  refresh.threads.clear()
  let failed = false
  try {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: mailKeys.lists(workspaceId) }, { throwOnError: true }),
      queryClient.invalidateQueries({ queryKey: mailKeys.counts(workspaceId) }, { throwOnError: true }),
      ...threads.map((threadId) =>
        queryClient.invalidateQueries({ queryKey: mailKeys.thread(workspaceId, threadId) }, { throwOnError: true }),
      ),
    ])
    refresh.delay = FIRST_RETRY_MS
  } catch {
    failed = true
    for (const threadId of threads) refresh.threads.add(threadId)
  } finally {
    refresh.running = false
  }
  if (failed) {
    later(queryClient, workspaceId, refresh, refresh.delay)
    refresh.delay = Math.min(refresh.delay * 2, MAX_RETRY_MS)
  } else {
    void run(queryClient, workspaceId, refresh)
  }
}

/**
 * Applies an event of the `mail` live topic: the changed thread, every thread list and the unread counts are read
 * again. Events that arrive while a refresh runs wait for it and go out together; a failed refresh is tried again
 * after a doubling wait (at most 30 s), never on a short fixed timer.
 */
export function emitMailEvent(queryClient: QueryClient, workspaceId: string, value: unknown): void {
  const event = parseMailEvent(value)
  if (!event) return
  const refresh = refreshFor(queryClient, workspaceId)
  refresh.threads.add(event.thread_id)
  queueMicrotask(() => void run(queryClient, workspaceId, refresh))
}

/** The live socket missed events: everything of mail is read again (the frame URLs stay). */
export function resyncMail(queryClient: QueryClient, workspaceId: string): void {
  void queryClient.invalidateQueries({ queryKey: mailKeys.all(workspaceId) })
}
