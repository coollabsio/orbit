import type { InfiniteData, QueryClient, QueryKey } from '@tanstack/react-query'
import type {
  MailCounts,
  MailFolder,
  MailThreadDetail,
  MailThreadPage,
  MailThreadSummary,
  UpdateMailThreadBody,
} from '@orbit/api/generated/types.gen'
import { mailKeys, type ThreadFilter } from './keys'

/** One change for one or many threads, as `PATCH threads/{id}` and `POST threads/bulk` take it. */
export type ThreadChange = UpdateMailThreadBody

type ThreadPages = InfiniteData<MailThreadPage, string | undefined>

/** The cached data a change touched, to put back when the write fails. */
export interface MailSnapshot {
  entries: Array<[QueryKey, unknown]>
}

/** Whether a thread belongs in a folder or label list (the server's rules in `list_threads`). */
export function fitsFilter(thread: MailThreadSummary, filter: ThreadFilter): boolean {
  if (filter.mailboxId && thread.mailbox_id !== filter.mailboxId) return false
  if (filter.labelId && !thread.labels.includes(filter.labelId)) return false
  if (filter.starred && !thread.starred) return false
  if (filter.folder) return thread.folder === filter.folder
  if (filter.labelId || filter.starred) return thread.folder === 'inbox' || thread.folder === 'archive'
  return thread.folder === 'inbox'
}

/** The thread after the change. A restore guesses the inbox; the response says where it went. */
export function changedThread(thread: MailThreadSummary, change: ThreadChange): MailThreadSummary {
  let folder: MailFolder = thread.folder
  if (change.folder) folder = change.folder
  else if (change.restore && thread.folder === 'trash') folder = 'inbox'
  const removed = new Set(change.remove_labels ?? [])
  const labels = thread.labels.filter((id) => !removed.has(id))
  for (const id of change.add_labels ?? []) if (!labels.includes(id)) labels.push(id)
  return {
    ...thread,
    folder,
    labels,
    unread: change.read == null ? thread.unread : !change.read,
    starred: change.starred ?? thread.starred,
  }
}

/** The filter of a folder or label list key; `null` for a search, whose rows stay until it is read again. */
const listFilter = (key: QueryKey): ThreadFilter | null => (key[3] === 'threads' ? (key[4] as ThreadFilter) : null)

function mapPages(data: ThreadPages | undefined, filter: ThreadFilter | null, replace: (thread: MailThreadSummary) => MailThreadSummary | null) {
  if (!data) return data
  return {
    ...data,
    pages: data.pages.map((page) => ({
      ...page,
      items: page.items.flatMap((item) => {
        const next = replace(item)
        if (next === null) return [item]
        return filter && !fitsFilter(next, filter) ? [] : [next]
      }),
    })),
  }
}

function snapshot(queryClient: QueryClient, workspaceId: string, threadIds: string[]): MailSnapshot {
  return {
    entries: [
      ...queryClient.getQueriesData({ queryKey: mailKeys.lists(workspaceId) }),
      ...queryClient.getQueriesData({ queryKey: mailKeys.counts(workspaceId) }),
      ...threadIds.flatMap((id) => queryClient.getQueriesData({ queryKey: mailKeys.thread(workspaceId, id) })),
    ],
  }
}

/** A cached copy of each thread, to count what the change does to the unread numbers. */
function cachedThreads(queryClient: QueryClient, workspaceId: string, ids: Set<string>): Map<string, MailThreadSummary> {
  const found = new Map<string, MailThreadSummary>()
  for (const id of ids) {
    const detail = queryClient.getQueryData<MailThreadDetail>(mailKeys.thread(workspaceId, id))
    if (detail?.thread) found.set(id, detail.thread)
  }
  for (const [, data] of queryClient.getQueriesData<ThreadPages>({ queryKey: mailKeys.lists(workspaceId) })) {
    for (const page of data?.pages ?? []) {
      for (const item of page.items) if (ids.has(item.id) && !found.has(item.id)) found.set(item.id, item)
    }
  }
  return found
}

function adjustCounts(counts: MailCounts, moves: Array<{ before: MailThreadSummary; after: MailThreadSummary }>): MailCounts {
  return {
    mailboxes: counts.mailboxes.map((mailbox) => {
      const next = { ...mailbox }
      for (const { before, after } of moves) {
        if (before.mailbox_id !== mailbox.mailbox_id) continue
        if (before.unread) next[before.folder] = Math.max(0, next[before.folder] - 1)
        if (after.unread) next[after.folder] += 1
      }
      return next
    }),
  }
}

/**
 * Applies a change to every cached copy of the threads at once: the lists (a row that no longer fits a folder or
 * label list leaves it), the open thread and the unread counts. Returns what to put back if the write fails.
 */
export function applyThreadChange(queryClient: QueryClient, workspaceId: string, threadIds: string[], change: ThreadChange): MailSnapshot {
  const saved = snapshot(queryClient, workspaceId, threadIds)
  const ids = new Set(threadIds)
  const moves = [...cachedThreads(queryClient, workspaceId, ids).values()].map((before) => ({ before, after: changedThread(before, change) }))

  const replace = (thread: MailThreadSummary) => (ids.has(thread.id) ? changedThread(thread, change) : null)
  for (const [key, data] of queryClient.getQueriesData<ThreadPages>({ queryKey: mailKeys.lists(workspaceId) })) {
    queryClient.setQueryData(key, mapPages(data, listFilter(key), replace))
  }
  for (const id of ids) {
    queryClient.setQueryData<MailThreadDetail>(mailKeys.thread(workspaceId, id), (detail) =>
      detail ? { ...detail, thread: changedThread(detail.thread, change) } : detail,
    )
  }
  queryClient.setQueryData<MailCounts>(mailKeys.counts(workspaceId), (counts) => (counts ? adjustCounts(counts, moves) : counts))
  return saved
}

export function restoreMailSnapshot(queryClient: QueryClient, saved: MailSnapshot): void {
  for (const [key, data] of saved.entries) queryClient.setQueryData(key, data)
}

/** Puts the server's rows of changed threads over the cached copies (the counts are read again instead). */
export function reconcileThreads(queryClient: QueryClient, workspaceId: string, threads: MailThreadSummary[]): void {
  const byId = new Map(threads.map((thread) => [thread.id, thread]))
  const replace = (thread: MailThreadSummary) => byId.get(thread.id) ?? null
  for (const [key, data] of queryClient.getQueriesData<ThreadPages>({ queryKey: mailKeys.lists(workspaceId) })) {
    queryClient.setQueryData(key, mapPages(data, listFilter(key), replace))
  }
  for (const thread of threads) {
    queryClient.setQueryData<MailThreadDetail>(mailKeys.thread(workspaceId, thread.id), (detail) =>
      detail ? { ...detail, thread } : detail,
    )
  }
}
