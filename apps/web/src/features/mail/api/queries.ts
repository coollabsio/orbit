import { keepPreviousData, useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import {
  createMailViewUrl,
  getMailAdmin,
  getMailCounts,
  getMailDomain,
  getMailMe,
  getMailTestAddress,
  downloadMailMessage,
  getMailThread,
  listFailedMailReceipts,
  listInstanceMailProviders,
  listWorkspaceMailProviders,
  listMailboxes,
  listMailDomains,
  listMailLabels,
  listMailThreads,
  previewPersonalMailboxes,
  searchMail,
} from '@/api/generated/sdk.gen'
import type { MailDomain, MailNamePattern, MailThreadPage } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { useContext } from 'react'
import { useWorkspace, WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { mailKeys, type ThreadFilter } from './keys'

/** Lists, threads and counts stay current through the `mail` live topic (`emitMailEvent`). */
const LIVE_STALE_MS = 5 * 60_000
/** A view URL works for 5 minutes; a frame that mounts later needs a new one. */
const VIEW_URL_STALE_MS = 4 * 60_000
/** The test message card asks for the result this often while it waits. */
const TEST_POLL_MS = 3000

function required<T>(data: T | undefined): T {
  if (data === undefined) throw new Error('The server sent an empty response.')
  return data
}

const useWorkspaceId = () => useWorkspace().workspace.id
/** The Admin pages can run without a workspace (an operator with none): instance hooks must not need one. */
const useOptionalWorkspaceId = () => useContext(WorkspaceContext)?.workspace.id ?? ''

const flatten = (data: { pages: MailThreadPage[] }) => data.pages.flatMap((page) => page.items)

/** The mailboxes (with their inbox unread count) and labels the user reads, and whether they manage mail. */
export function useMailMe() {
  const workspaceId = useWorkspaceId()
  return useQuery({
    queryKey: mailKeys.me(workspaceId),
    queryFn: async () => required((await getMailMe({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })).data),
  })
}

/** A folder or label list, newest first, in cursor pages; `data` is the rows of every loaded page. */
export function useThreads(filter: ThreadFilter, enabled = true) {
  const workspaceId = useWorkspaceId()
  return useInfiniteQuery({
    queryKey: mailKeys.threads(workspaceId, filter),
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }) => {
      const { data } = await listMailThreads({
        client: apiClient,
        path: { workspace_id: workspaceId },
        query: {
          mailbox_id: filter.mailboxId,
          folder: filter.folder,
          label_id: filter.labelId,
          starred: filter.starred || undefined,
          cursor: pageParam,
        },
        throwOnError: true,
      })
      return required(data)
    },
    getNextPageParam: (last) => last.cursor ?? undefined,
    select: flatten,
    enabled,
    staleTime: LIVE_STALE_MS,
  })
}

/** A search with the operators of `parse_query`; idle while `query` is blank. */
export function useMailSearch(query: string) {
  const workspaceId = useWorkspaceId()
  const q = query.trim()
  return useInfiniteQuery({
    queryKey: mailKeys.search(workspaceId, q),
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }) =>
      required((await searchMail({ client: apiClient, path: { workspace_id: workspaceId }, query: { q, cursor: pageParam }, throwOnError: true })).data),
    getNextPageParam: (last) => last.cursor ?? undefined,
    select: flatten,
    enabled: q !== '',
    placeholderData: keepPreviousData,
    staleTime: LIVE_STALE_MS,
  })
}

/** A thread with its messages, oldest first. */
export function useThread(threadId: string | null | undefined) {
  const workspaceId = useWorkspaceId()
  return useQuery({
    queryKey: mailKeys.thread(workspaceId, threadId ?? ''),
    queryFn: async () =>
      required((await getMailThread({ client: apiClient, path: { workspace_id: workspaceId, thread_id: threadId! }, throwOnError: true })).data),
    enabled: Boolean(threadId),
    staleTime: LIVE_STALE_MS,
  })
}

/** Unread threads of each folder of each mailbox the user reads. */
export function useMailCounts() {
  const workspaceId = useWorkspaceId()
  return useQuery({
    queryKey: mailKeys.counts(workspaceId),
    queryFn: async () => required((await getMailCounts({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })).data),
    staleTime: LIVE_STALE_MS,
  })
}

/** Unread threads in the Inbox of every mailbox the user reads (the Mail badge, like a mail app's dock badge). */
export function useMailInboxUnread(): number {
  return useMailCounts().data?.mailboxes.reduce((total, mailbox) => total + mailbox.inbox, 0) ?? 0
}

/**
 * The short-lived URL of a message's sandboxed view document (`{ url, expires_at }`). It is not read again while the
 * frame shows it (a new URL reloads the frame); a frame that mounts after 4 minutes gets a new one.
 */
export function useViewUrl(messageId: string | null | undefined, loadImages: boolean) {
  const workspaceId = useWorkspaceId()
  return useQuery({
    queryKey: mailKeys.viewUrl(workspaceId, messageId ?? '', loadImages),
    queryFn: async () =>
      required(
        (
          await createMailViewUrl({
            client: apiClient,
            path: { workspace_id: workspaceId, message_id: messageId! },
            body: { load_images: loadImages },
            throwOnError: true,
          })
        ).data,
      ),
    enabled: Boolean(messageId),
    staleTime: VIEW_URL_STALE_MS,
    gcTime: VIEW_URL_STALE_MS,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  })
}

/** **Show original**: the message as received, as text (at most `RAW_TEXT_CHARS`; the `.eml` download has it all). */
export const RAW_TEXT_CHARS = 1_000_000

export function useRawMessage(messageId: string | null | undefined) {
  const workspaceId = useWorkspaceId()
  return useQuery({
    queryKey: mailKeys.raw(workspaceId, messageId ?? ''),
    queryFn: async () => {
      const { data } = await downloadMailMessage({
        client: apiClient,
        path: { workspace_id: workspaceId, message_id: messageId! },
        parseAs: 'text',
        throwOnError: true,
      })
      const text = data as unknown as string
      return { text: text.slice(0, RAW_TEXT_CHARS), truncated: text.length > RAW_TEXT_CHARS }
    },
    enabled: Boolean(messageId),
    staleTime: Infinity,
    gcTime: 60_000,
  })
}

export function useMailDomains() {
  const workspaceId = useWorkspaceId()
  return useQuery({
    queryKey: mailKeys.domains(workspaceId),
    queryFn: async () => required((await listMailDomains({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })).data),
  })
}

/** One domain with its records; `refetchInterval` while a DNS check runs. */
/** `refetchInterval` may be a function of the cached domain and when it was read, to poll until a change shows. */
export function useMailDomain(
  domainId: string | null | undefined,
  refetchInterval: number | false | ((data: MailDomain | undefined, updatedAt: number) => number | false) = false,
) {
  const workspaceId = useWorkspaceId()
  return useQuery({
    queryKey: mailKeys.domain(workspaceId, domainId ?? ''),
    queryFn: async () =>
      required((await getMailDomain({ client: apiClient, path: { workspace_id: workspaceId, domain_id: domainId! }, throwOnError: true })).data),
    enabled: Boolean(domainId),
    refetchInterval:
      typeof refetchInterval === 'function'
        ? (query) => refetchInterval(query.state.data, query.state.dataUpdatedAt)
        : refetchInterval,
  })
}

/** Managers see every mailbox; members their own. */
export function useMailboxes() {
  const workspaceId = useWorkspaceId()
  return useQuery({
    queryKey: mailKeys.mailboxes(workspaceId),
    queryFn: async () => required((await listMailboxes({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })).data),
  })
}

/**
 * **Create a mailbox for each member**: the proposed address of each member without a personal mailbox, with the
 * conflicts marked. Read again when the domain or the pattern changes; idle without a domain.
 */
export function useBulkPersonalPreview(domainId: string | null | undefined, pattern: MailNamePattern) {
  const workspaceId = useWorkspaceId()
  return useQuery({
    queryKey: mailKeys.bulkPreview(workspaceId, domainId ?? '', pattern),
    queryFn: async () =>
      required(
        (
          await previewPersonalMailboxes({
            client: apiClient,
            path: { workspace_id: workspaceId },
            body: { domain_id: domainId!, pattern },
            throwOnError: true,
          })
        ).data,
      ),
    enabled: Boolean(domainId),
  })
}

export function useMailLabels() {
  const workspaceId = useWorkspaceId()
  return useQuery({
    queryKey: mailKeys.labels(workspaceId),
    queryFn: async () => required((await listMailLabels({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })).data),
  })
}

/** Messages that failed to file or are stuck (`{ count, receipts }`, newest 100). Managers only. */
export function useFailedReceipts() {
  const workspaceId = useWorkspaceId()
  return useQuery({
    queryKey: mailKeys.receipts(workspaceId),
    queryFn: async () =>
      required((await listFailedMailReceipts({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })).data),
  })
}

/** Whose providers: the root user's (Admin → Mail) or the workspace's, which also lists the instance ones. */
export type ProviderScope = 'instance' | 'workspace'

/** Mail providers with their setup (`{ providers, workspace_providers_enabled }`). */
export function useMailProviders(scope: ProviderScope) {
  const workspaceId = useOptionalWorkspaceId()
  return useQuery({
    queryKey: scope === 'instance' ? queryKeys.adminMailProviders : mailKeys.providers(workspaceId),
    queryFn: async () =>
      required(
        scope === 'instance'
          ? (await listInstanceMailProviders({ client: apiClient, throwOnError: true })).data
          : (await listWorkspaceMailProviders({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })).data,
      ),
  })
}

/** The Admin mail page: the switch, the listener, the certificate and the PTR check (looked up live). Root only. */
export function useAdminMail() {
  return useQuery({
    queryKey: queryKeys.adminMail,
    queryFn: async () => required((await getMailAdmin({ client: apiClient, throwOnError: true })).data),
  })
}

/** The result of a test address, asked for every 3 s while it waits for the message. */
export function useTestAddress(testId: string | null | undefined) {
  return useQuery({
    queryKey: queryKeys.adminMailTest(testId ?? ''),
    queryFn: async () => required((await getMailTestAddress({ client: apiClient, path: { test_id: testId! }, throwOnError: true })).data),
    enabled: Boolean(testId),
    refetchInterval: (query) => (query.state.data?.status === 'waiting' ? TEST_POLL_MS : false),
  })
}
