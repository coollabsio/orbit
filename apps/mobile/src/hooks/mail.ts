/** The mail queries and writes the screens share. Each reads its workspace and client from `useOrbit()`. */
import { type InfiniteData, type QueryClient, useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiProblem } from '@orbit/api/problem'
import { router } from 'expo-router'
import { useSyncExternalStore } from 'react'
import { Alert } from 'react-native'

import { useAuthOptions } from './account'
import { mailKeys, type ThreadFilter } from '../data/keys'
import { changedThread, filterOfKey, patchRows, type ThreadChange } from '../data/mail'
import * as mail from '../data/mail-api'
import type { MailThreadDetail, MailThreadPage, MailThreadSummary } from '../data/mail-api'
import { useOrbit } from '../orbit-context'
import { errorMessage } from '../session-context'

/** Lists, threads and counts stay current through the live `mail` topic (`src/live.ts`). */
const LIVE_STALE_MS = 5 * 60_000
/** A view token works for 5 minutes; a message that opens later fetches its document again. */
const VIEW_STALE_MS = 4 * 60_000

/** Mail is on for this server (Admin → Mail). Off, and until the options load: no Mail tab. */
export function useMailEnabled(): boolean {
  return useAuthOptions().data?.mail_enabled === true
}

export function useMailMe() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: mailKeys.me(workspaceId), queryFn: () => mail.mailMe(client, workspaceId) })
}

export function useMailCounts(enabled = true) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: mailKeys.counts(workspaceId), queryFn: () => mail.mailCounts(client, workspaceId), enabled, staleTime: LIVE_STALE_MS })
}

/** Unread threads in the Inbox of every mailbox the user reads: the Mail item's badge. */
export function useMailBadge(): number {
  const counts = useMailCounts(useMailEnabled())
  return counts.data?.mailboxes.reduce((total, mailbox) => total + mailbox.inbox, 0) ?? 0
}

const flatten = (data: InfiniteData<MailThreadPage>) => data.pages.flatMap((page) => page.items)

/** A folder list in cursor pages; `data` is the rows of every page loaded. */
export function useThreads(filter: ThreadFilter, enabled: boolean) {
  const { client, workspaceId } = useOrbit()
  return useInfiniteQuery({
    queryKey: mailKeys.threads(workspaceId, filter),
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => mail.threadPage(client, workspaceId, filter, pageParam),
    getNextPageParam: (last) => last.cursor ?? undefined,
    select: flatten,
    enabled,
    staleTime: LIVE_STALE_MS,
  })
}

/** A search; idle while `query` is blank. */
export function useMailSearch(query: string) {
  const { client, workspaceId } = useOrbit()
  const q = query.trim()
  return useInfiniteQuery({
    queryKey: mailKeys.search(workspaceId, q),
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => mail.searchPage(client, workspaceId, q, pageParam),
    getNextPageParam: (last) => last.cursor ?? undefined,
    select: flatten,
    enabled: q !== '',
    staleTime: LIVE_STALE_MS,
  })
}

/** A thread with its messages, oldest first; idle without an id (a new message has no thread). */
export function useThread(threadId: string) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: mailKeys.thread(workspaceId, threadId), queryFn: () => mail.thread(client, workspaceId, threadId), enabled: threadId !== '', staleTime: LIVE_STALE_MS })
}

export function useDrafts(enabled = true) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: mailKeys.drafts(workspaceId), queryFn: () => mail.drafts(client, workspaceId), enabled, staleTime: LIVE_STALE_MS })
}

export function useOutbound(view: 'scheduled' | 'outbox', enabled = true) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: mailKeys.outbound(workspaceId, view), queryFn: () => mail.outbound(client, workspaceId, view), enabled, staleTime: LIVE_STALE_MS })
}

export function useFromAddresses(threadId?: string) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: mailKeys.fromAddresses(workspaceId, threadId), queryFn: () => mail.fromAddresses(client, workspaceId, threadId) })
}

export function useContacts(query: string) {
  const { client, workspaceId } = useOrbit()
  const q = query.trim()
  return useQuery({ queryKey: mailKeys.contacts(workspaceId, q), queryFn: () => mail.contacts(client, workspaceId, q), enabled: q !== '', staleTime: LIVE_STALE_MS })
}

/** Owners and admins: whether sending is paused for the workspace. */
export function useSendPolicy() {
  const { client, workspaceId, workspace } = useOrbit()
  return useQuery({
    queryKey: mailKeys.sendPolicy(workspaceId),
    queryFn: () => mail.sendPolicy(client, workspaceId),
    enabled: workspace.role === 'owner' || workspace.role === 'admin',
  })
}

/** The view document of a message, read once while it shows (a new token would reload the frame). */
export function useViewDocument(messageId: string, loadImages: boolean) {
  const { client, workspaceId, session } = useOrbit()
  return useQuery({
    queryKey: mailKeys.view(workspaceId, messageId, loadImages),
    queryFn: () => mail.viewDocument(client, session.server, workspaceId, messageId, loadImages),
    staleTime: VIEW_STALE_MS,
    gcTime: VIEW_STALE_MS,
    refetchOnReconnect: false,
    refetchOnWindowFocus: false,
  })
}

// ---- thread writes

type Pages = InfiniteData<MailThreadPage, string | undefined>

/** Puts `replace` over every cached list row and the open thread; a row that no longer fits its list leaves it. */
function patchThreads(queryClient: QueryClient, workspaceId: string, ids: Set<string>, replace: (thread: MailThreadSummary) => MailThreadSummary | null) {
  for (const [key, data] of queryClient.getQueriesData<Pages>({ queryKey: mailKeys.lists(workspaceId) })) {
    if (!data) continue
    const filter = filterOfKey(key)
    queryClient.setQueryData<Pages>(key, { ...data, pages: data.pages.map((page) => ({ ...page, items: patchRows(page.items, filter, replace) })) })
  }
  for (const id of ids) {
    queryClient.setQueryData<MailThreadDetail>(mailKeys.thread(workspaceId, id), (detail) => {
      const next = detail && replace(detail.thread)
      return detail && next ? { ...detail, thread: next } : detail
    })
  }
}

/**
 * Read, star, folder and labels of one thread: every cached copy changes at once, a failure puts them back, the
 * server's row replaces them on success, and the counts are read again.
 */
export function useUpdateThread() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationKey: [...mailKeys.all(workspaceId), 'threads'],
    mutationFn: ({ threadId, change }: { threadId: string; change: ThreadChange }) => mail.updateThread(client, workspaceId, threadId, change),
    onMutate: async ({ threadId, change }) => {
      await Promise.all([queryClient.cancelQueries({ queryKey: mailKeys.lists(workspaceId) }), queryClient.cancelQueries({ queryKey: mailKeys.thread(workspaceId, threadId) })])
      const saved = [...queryClient.getQueriesData({ queryKey: mailKeys.lists(workspaceId) }), ...queryClient.getQueriesData({ queryKey: mailKeys.thread(workspaceId, threadId) })]
      patchThreads(queryClient, workspaceId, new Set([threadId]), (thread) => (thread.id === threadId ? changedThread(thread, change) : null))
      return { saved }
    },
    onError: (_error, _variables, context) => {
      for (const [key, data] of context?.saved ?? []) queryClient.setQueryData(key, data)
    },
    onSuccess: (thread) => patchThreads(queryClient, workspaceId, new Set([thread.id]), (row) => (row.id === thread.id ? thread : null)),
    onSettled: () => queryClient.invalidateQueries({ queryKey: mailKeys.counts(workspaceId) }),
  })
}

/** After a send, a cancel or a draft change: the lists that show drafts and waiting mail are read again. */
export function refreshSendViews(queryClient: QueryClient, workspaceId: string) {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: mailKeys.drafts(workspaceId) }),
    queryClient.invalidateQueries({ queryKey: mailKeys.outboundAll(workspaceId) }),
    queryClient.invalidateQueries({ queryKey: mailKeys.lists(workspaceId) }),
  ])
}

// ---- starting a draft

export type DraftStart = { mode: 'new' } | { mode: 'reply' | 'reply_all' | 'forward'; threadId: string }

/**
 * Starts a message and opens it in the compose screen. A new one is from the first address that can send (else the
 * first: Send then says why); a reply or forward is the thread's one draft, which the server makes or gives back.
 */
export function useStartDraft() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (start: DraftStart) => {
      if (start.mode !== 'new') return mail.createDraft(client, workspaceId, { mode: start.mode, thread_id: start.threadId })
      const addresses = await queryClient.fetchQuery({ queryKey: mailKeys.fromAddresses(workspaceId), queryFn: () => mail.fromAddresses(client, workspaceId), staleTime: 60_000 })
      const from = addresses.find((address) => address.ready === 'ready') ?? addresses[0]
      if (!from) throw new Error('You have no address to send from. Ask an admin for a mailbox.')
      return mail.createDraft(client, workspaceId, { mode: 'new', from_address_id: from.address_id })
    },
    onSuccess: (draft) => {
      queryClient.setQueryData(mailKeys.draft(workspaceId, draft.id), draft)
      router.push({ pathname: '/mail/compose', params: { draft: draft.id } })
    },
    onError: (error) =>
      Alert.alert(
        'The message could not be started.',
        error instanceof ApiProblem && error.code === 'draft_sending' ? 'Your reply to this conversation is being sent. Undo it in the Outbox to change it.' : errorMessage(error),
      ),
  })
}

// ---- undo send

/** A message the user just sent, held by the server for the undo time (web `UndoSendToast`). */
export interface PendingSend {
  outboundId: string
  releaseAt: number
}

let pending: PendingSend | null = null
const listeners = new Set<() => void>()
const subscribe = (listener: () => void) => {
  listeners.add(listener)
  return () => void listeners.delete(listener)
}

export function setPendingSend(next: PendingSend | null) {
  pending = next
  for (const listener of listeners) listener()
}

export function usePendingSend(): PendingSend | null {
  return useSyncExternalStore(subscribe, () => pending)
}
