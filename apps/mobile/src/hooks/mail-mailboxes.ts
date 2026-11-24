/** Settings → Mail → Mailboxes, Mail labels, Suppressions and Failed messages: queries and writes, with the web's refreshes. */
import { type QueryKey, useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { CreateMailboxBody, CreatePersonalMailboxesBody, MailboxStatus } from '@orbit/api/generated/types.gen'

import { mailKeys } from '../data/keys'
import * as api from '../data/mail-mailboxes-api'
import { useOrbit } from '../orbit-context'

const keys = api.mailSettingsKeys

export function useMailDomainList() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.domains(workspaceId), queryFn: () => api.domains(client, workspaceId) })
}

export function useMailboxList() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.mailboxes(workspaceId), queryFn: () => api.mailboxes(client, workspaceId) })
}

/** Read again when the domain or the pattern changes; idle without a domain. */
export function useBulkPersonalPreview(domainId: string, pattern: api.MailNamePattern) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.bulkPreview(workspaceId, domainId, pattern), queryFn: () => api.bulkPreview(client, workspaceId, domainId, pattern), enabled: domainId !== '' })
}

/** A mailbox change also changes the user's mailbox list, the counts, the domains' address counts and the sender names of the From picker. */
export function useMailboxWrites() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const refresh = (extra: QueryKey[] = []) => () => {
    for (const queryKey of [keys.mailboxes(workspaceId), mailKeys.me(workspaceId), mailKeys.counts(workspaceId), keys.domains(workspaceId), mailKeys.fromAddresses(workspaceId).slice(0, 3), ...extra]) void queryClient.invalidateQueries({ queryKey })
  }
  const onSettled = refresh()
  // the threads of a deleted mailbox, or of one a member no longer reads, leave their lists
  const withLists = refresh([mailKeys.lists(workspaceId)])
  type InMailbox = { mailboxId: string }
  return {
    create: useMutation({ mutationFn: (body: CreateMailboxBody) => api.createMailbox(client, workspaceId, body), onSettled }),
    rename: useMutation({ mutationFn: ({ mailboxId, name }: InMailbox & { name: string }) => api.renameMailbox(client, workspaceId, mailboxId, name), onSettled }),
    remove: useMutation({ mutationFn: (mailboxId: string) => api.deleteMailbox(client, workspaceId, mailboxId), onSettled: withLists }),
    setStatus: useMutation({ mutationFn: ({ mailboxId, status }: InMailbox & { status: MailboxStatus }) => api.setStatus(client, workspaceId, mailboxId, status), onSettled }),
    addMember: useMutation({ mutationFn: ({ mailboxId, membershipId }: InMailbox & { membershipId: string }) => api.addMember(client, workspaceId, mailboxId, membershipId), onSettled }),
    removeMember: useMutation({ mutationFn: ({ mailboxId, membershipId }: InMailbox & { membershipId: string }) => api.removeMember(client, workspaceId, mailboxId, membershipId), onSettled: withLists }),
    addAlias: useMutation({
      mutationFn: ({ mailboxId, domainId, localPart, senderName }: InMailbox & { domainId: string; localPart: string; senderName: string | null }) =>
        api.addAlias(client, workspaceId, mailboxId, domainId, localPart, senderName),
      onSettled,
    }),
    setSenderName: useMutation({
      mutationFn: ({ mailboxId, addressId, senderName }: InMailbox & { addressId: string; senderName: string | null }) =>
        api.setSenderName(client, workspaceId, mailboxId, addressId, senderName),
      onSettled,
    }),
    removeAddress: useMutation({ mutationFn: ({ mailboxId, addressId }: InMailbox & { addressId: string }) => api.removeAddress(client, workspaceId, mailboxId, addressId), onSettled }),
    bulk: useMutation({ mutationFn: (body: CreatePersonalMailboxesBody) => api.bulkCreate(client, workspaceId, body), onSettled }),
  }
}

export function useMailLabelList() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.labels(workspaceId), queryFn: () => api.labels(client, workspaceId) })
}

/** A label change shows in the settings list and the mail app's sidebar (`me`); a deleted one leaves every thread. */
export function useMailLabelWrites() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const refresh = (extra: QueryKey[] = []) => () => {
    for (const queryKey of [keys.labels(workspaceId), mailKeys.me(workspaceId), ...extra]) void queryClient.invalidateQueries({ queryKey })
  }
  const onSettled = refresh()
  return {
    create: useMutation({ mutationFn: ({ name, color }: { name: string; color: string }) => api.createLabel(client, workspaceId, { name, color }), onSettled }),
    update: useMutation({ mutationFn: ({ labelId, name, color }: { labelId: string; name: string; color: string }) => api.updateLabel(client, workspaceId, labelId, { name, color }), onSettled }),
    remove: useMutation({ mutationFn: (labelId: string) => api.deleteLabel(client, workspaceId, labelId), onSettled: refresh([mailKeys.lists(workspaceId), ['mail', workspaceId, 'thread']]) }),
  }
}

/** Suppressed addresses in cursor pages; `data` is the rows of every page loaded. */
export function useSuppressions(q: string) {
  const { client, workspaceId } = useOrbit()
  const query = q.trim() || undefined
  return useInfiniteQuery({
    queryKey: [...keys.suppressions(workspaceId), query ?? null],
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => api.suppressionPage(client, workspaceId, query, pageParam),
    getNextPageParam: (last) => last.cursor ?? undefined,
    select: (data) => data.pages.flatMap((page) => page.items),
  })
}

export function useRemoveSuppression() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (address: string) => api.removeSuppression(client, workspaceId, address),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: keys.suppressions(workspaceId) }),
  })
}

export function useFailedReceipts() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.receipts(workspaceId), queryFn: () => api.failedReceipts(client, workspaceId) })
}

export function useRetryReceipt() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (receiptId: string) => api.retryReceipt(client, workspaceId, receiptId),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: keys.receipts(workspaceId) }),
  })
}
