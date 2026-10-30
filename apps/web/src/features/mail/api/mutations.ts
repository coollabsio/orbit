import { type QueryClient, type QueryKey, useMutation, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import {
  addMailboxAlias,
  addMailboxMember,
  checkMailDomain,
  createMailbox,
  createMailDomain,
  createMailLabel,
  createInstanceMailProvider,
  createWorkspaceMailProvider,
  deleteInstanceMailProvider,
  deleteWorkspaceMailProvider,
  createMailTestAddress,
  createPersonalMailboxes,
  deleteMailbox,
  deleteMailDomain,
  deleteMailLabel,
  removeMailboxAddress,
  removeMailboxMember,
  retryMailReceipt,
  setMailAdmin,
  setMailboxStatus,
  setMailDomainCatchAll,
  setMailDomainEnabled,
  setMailDomainRoute,
  setMailSenderPref,
  updateInstanceMailProvider,
  updateMailbox,
  updateWorkspaceMailProvider,
  updateMailLabel,
  updateMailThread,
  updateMailThreads,
} from '@/api/generated/sdk.gen'
import type {
  CreateMailboxBody,
  CreateMailLabelBody,
  CreateMailProviderBody,
  CreatePersonalMailboxesBody,
  MailboxAliasBody,
  MailboxStatus,
  MailSenderPrefBody,
  MailThreadSummary,
  SetMailAdminBody,
  UpdateMailLabelBody,
  UpdateMailProviderBody,
} from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { useContext } from 'react'
import { useWorkspace, WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { mailKeys } from './keys'
import type { ProviderScope } from './queries'
import { applyThreadChange, type MailSnapshot, reconcileThreads, restoreMailSnapshot, type ThreadChange } from './optimistic'

export type { ThreadChange } from './optimistic'

function required<T>(data: T | undefined): T {
  if (data === undefined) throw new Error('The server sent an empty response.')
  return data
}

const useWorkspaceId = () => useWorkspace().workspace.id
/** The Admin pages can run without a workspace (an operator with none): instance hooks must not need one. */
const useOptionalWorkspaceId = () => useContext(WorkspaceContext)?.workspace.id ?? ''

const invalidate = (queryClient: QueryClient, keys: QueryKey[]) =>
  Promise.all(keys.map((queryKey) => queryClient.invalidateQueries({ queryKey })))

/**
 * The optimistic write behind `useUpdateThread` and `useBulkUpdateThreads`: every cached copy changes at once, a
 * failure puts them back, the server's rows replace them on success, and the counts are read again. Its mutation key
 * lets `emitMailEvent` hold a live refresh until the write is done.
 */
function useThreadWrite<Variables>(
  threadIds: (variables: Variables) => string[],
  change: (variables: Variables) => ThreadChange,
  send: (workspaceId: string, variables: Variables) => Promise<MailThreadSummary[]>,
) {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation<MailThreadSummary[], Error, Variables, { snapshot: MailSnapshot }>({
    mutationKey: [...mailKeys.all(workspaceId), 'threads'],
    mutationFn: (variables) => send(workspaceId, variables),
    onMutate: async (variables) => {
      const ids = threadIds(variables)
      await Promise.all([
        queryClient.cancelQueries({ queryKey: mailKeys.lists(workspaceId) }),
        queryClient.cancelQueries({ queryKey: mailKeys.counts(workspaceId) }),
        ...ids.map((id) => queryClient.cancelQueries({ queryKey: mailKeys.thread(workspaceId, id) })),
      ])
      return { snapshot: applyThreadChange(queryClient, workspaceId, ids, change(variables)) }
    },
    onError: (_error, variables, context) => {
      if (context) restoreMailSnapshot(queryClient, context.snapshot)
      void invalidate(queryClient, [mailKeys.lists(workspaceId), ...threadIds(variables).map((id) => mailKeys.thread(workspaceId, id))])
    },
    onSuccess: (threads) => reconcileThreads(queryClient, workspaceId, threads),
    onSettled: () => queryClient.invalidateQueries({ queryKey: mailKeys.counts(workspaceId) }),
  })
}

/** Read, star, folder (or `restore: true` out of the trash) and labels of one thread. */
export function useUpdateThread() {
  return useThreadWrite<{ threadId: string; change: ThreadChange }>(
    ({ threadId }) => [threadId],
    ({ change }) => change,
    async (workspaceId, { threadId, change }) => [
      required(
        (await updateMailThread({ client: apiClient, path: { workspace_id: workspaceId, thread_id: threadId }, body: change, throwOnError: true }))
          .data,
      ),
    ],
  )
}

/** The same change for 1–100 threads; threads the user does not read are left alone. */
export function useBulkUpdateThreads() {
  return useThreadWrite<{ threadIds: string[]; change: ThreadChange }>(
    ({ threadIds }) => threadIds,
    ({ change }) => change,
    async (workspaceId, { threadIds, change }) =>
      required(
        (await updateMailThreads({ client: apiClient, path: { workspace_id: workspaceId }, body: { ...change, ids: threadIds }, throwOnError: true }))
          .data,
      ).items,
  )
}

/** **Always for this sender**: remote images of `address` load at once when DMARC passes. */
export function useSetSenderPref() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async (body: MailSenderPrefBody) =>
      required((await setMailSenderPref({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })).data),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: mailKeys.threadDetails(workspaceId) }),
  })
}

// Domains

export function useCreateDomain() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    /** `providerId`: receive through that provider; absent: this server's SMTP listener. */
    mutationFn: async ({ name, providerId }: { name: string; providerId?: string | null }) =>
      required(
        (
          await createMailDomain({
            client: apiClient,
            path: { workspace_id: workspaceId },
            body: { name, provider_id: providerId ?? null },
            throwOnError: true,
          })
        ).data,
      ),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: mailKeys.domains(workspaceId) }),
  })
}

/** **Check now**: queues a DNS check (202); the domain shows the result when the job ran (`useMailDomain` polling). */
export function useCheckDomain() {
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async (domainId: string) => {
      await checkMailDomain({ client: apiClient, path: { workspace_id: workspaceId, domain_id: domainId }, throwOnError: true })
    },
  })
}

export function useDeleteDomain() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async (domainId: string) => {
      await deleteMailDomain({ client: apiClient, path: { workspace_id: workspaceId, domain_id: domainId }, throwOnError: true })
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: mailKeys.domains(workspaceId) }),
  })
}

/** The mailbox that gets mail for unknown addresses of the domain; `null` refuses it. */
export function useSetCatchAll() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async ({ domainId, mailboxId }: { domainId: string; mailboxId: string | null }) =>
      required(
        (
          await setMailDomainCatchAll({
            client: apiClient,
            path: { workspace_id: workspaceId, domain_id: domainId },
            body: { mailbox_id: mailboxId },
            throwOnError: true,
          })
        ).data,
      ),
    onSettled: () => queryClient.invalidateQueries({ queryKey: mailKeys.domains(workspaceId) }),
  })
}

/** Where a domain's mail arrives: a provider, or this server's SMTP listener with `null`. */
export function useSetDomainRoute() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async ({ domainId, providerId }: { domainId: string; providerId: string | null }) =>
      required(
        (
          await setMailDomainRoute({
            client: apiClient,
            path: { workspace_id: workspaceId, domain_id: domainId },
            body: { provider_id: providerId },
            throwOnError: true,
          })
        ).data,
      ),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: mailKeys.domains(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: mailKeys.providers(workspaceId) })
    },
  })
}

export function useSetDomainEnabled() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async ({ domainId, enabled }: { domainId: string; enabled: boolean }) =>
      required(
        (
          await setMailDomainEnabled({
            client: apiClient,
            path: { workspace_id: workspaceId, domain_id: domainId },
            body: { enabled },
            throwOnError: true,
          })
        ).data,
      ),
    onSettled: () => queryClient.invalidateQueries({ queryKey: mailKeys.domains(workspaceId) }),
  })
}

// Mailboxes

/** A mailbox change also changes the user's mailbox list, the counts and the domains' address counts. */
const mailboxKeys = (workspaceId: string) => [
  mailKeys.mailboxes(workspaceId),
  mailKeys.me(workspaceId),
  mailKeys.counts(workspaceId),
  mailKeys.domains(workspaceId),
]

function useMailboxWrite<Variables, Result>(
  send: (workspaceId: string, variables: Variables) => Promise<Result>,
  extra: (workspaceId: string) => QueryKey[] = () => [],
) {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: (variables: Variables) => send(workspaceId, variables),
    onSettled: () => invalidate(queryClient, [...mailboxKeys(workspaceId), ...extra(workspaceId)]),
  })
}

export function useCreateMailbox() {
  return useMailboxWrite(async (workspaceId, body: CreateMailboxBody) =>
    required((await createMailbox({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })).data),
  )
}

/** Renames a mailbox. */
export function useUpdateMailbox() {
  return useMailboxWrite(async (workspaceId, { mailboxId, name }: { mailboxId: string; name: string }) =>
    required(
      (await updateMailbox({ client: apiClient, path: { workspace_id: workspaceId, mailbox_id: mailboxId }, body: { name }, throwOnError: true }))
        .data,
    ),
  )
}

export function useDeleteMailbox() {
  return useMailboxWrite(
    async (workspaceId, mailboxId: string) => {
      await deleteMailbox({ client: apiClient, path: { workspace_id: workspaceId, mailbox_id: mailboxId }, throwOnError: true })
    },
    // its threads leave every list
    (workspaceId) => [mailKeys.lists(workspaceId)],
  )
}

/** Suspend (refuses new mail, threads stay readable) or resume. */
export function useSetMailboxStatus() {
  return useMailboxWrite(async (workspaceId, { mailboxId, status }: { mailboxId: string; status: MailboxStatus }) =>
    required(
      (
        await setMailboxStatus({ client: apiClient, path: { workspace_id: workspaceId, mailbox_id: mailboxId }, body: { status }, throwOnError: true })
      ).data,
    ),
  )
}

export function useAddMailboxMember() {
  return useMailboxWrite(async (workspaceId, { mailboxId, membershipId }: { mailboxId: string; membershipId: string }) =>
    required(
      (
        await addMailboxMember({
          client: apiClient,
          path: { workspace_id: workspaceId, mailbox_id: mailboxId },
          body: { membership_id: membershipId },
          throwOnError: true,
        })
      ).data,
    ),
  )
}

export function useRemoveMailboxMember() {
  return useMailboxWrite(
    async (workspaceId, { mailboxId, membershipId }: { mailboxId: string; membershipId: string }) =>
      required(
        (
          await removeMailboxMember({
            client: apiClient,
            path: { workspace_id: workspaceId, mailbox_id: mailboxId, membership_id: membershipId },
            throwOnError: true,
          })
        ).data,
      ),
    // a member who no longer reads the mailbox loses its threads
    (workspaceId) => [mailKeys.lists(workspaceId)],
  )
}

export function useAddMailboxAlias() {
  return useMailboxWrite(async (workspaceId, { mailboxId, ...body }: MailboxAliasBody & { mailboxId: string }) =>
    required(
      (await addMailboxAlias({ client: apiClient, path: { workspace_id: workspaceId, mailbox_id: mailboxId }, body, throwOnError: true })).data,
    ),
  )
}

export function useRemoveMailboxAddress() {
  return useMailboxWrite(async (workspaceId, { mailboxId, addressId }: { mailboxId: string; addressId: string }) =>
    required(
      (
        await removeMailboxAddress({
          client: apiClient,
          path: { workspace_id: workspaceId, mailbox_id: mailboxId, address_id: addressId },
          throwOnError: true,
        })
      ).data,
    ),
  )
}

/** Creates the personal mailboxes, with the addresses the admin edited in `overrides`. */
export function useBulkPersonal() {
  return useMailboxWrite(async (workspaceId, body: CreatePersonalMailboxesBody) =>
    required((await createPersonalMailboxes({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })).data),
  )
}

// Labels

/** A label change shows in the settings list and the mail app's sidebar (`me`). */
function useLabelWrite<Variables, Result>(send: (workspaceId: string, variables: Variables) => Promise<Result>, extra: (workspaceId: string) => QueryKey[] = () => []) {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: (variables: Variables) => send(workspaceId, variables),
    onSettled: () => invalidate(queryClient, [mailKeys.labels(workspaceId), mailKeys.me(workspaceId), ...extra(workspaceId)]),
  })
}

export function useCreateMailLabel() {
  return useLabelWrite(async (workspaceId, body: CreateMailLabelBody) =>
    required((await createMailLabel({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })).data),
  )
}

export function useUpdateMailLabel() {
  return useLabelWrite(async (workspaceId, { labelId, ...body }: UpdateMailLabelBody & { labelId: string }) =>
    required((await updateMailLabel({ client: apiClient, path: { workspace_id: workspaceId, label_id: labelId }, body, throwOnError: true })).data),
  )
}

export function useDeleteMailLabel() {
  return useLabelWrite(
    async (workspaceId, labelId: string) => {
      await deleteMailLabel({ client: apiClient, path: { workspace_id: workspaceId, label_id: labelId }, throwOnError: true })
    },
    // the label leaves every thread that had it
    (workspaceId) => [mailKeys.lists(workspaceId), mailKeys.threadDetails(workspaceId)],
  )
}

// Failed messages

/** Files a failed or stuck message again (204). */
export function useRetryReceipt() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async (receiptId: string) => {
      await retryMailReceipt({ client: apiClient, path: { workspace_id: workspaceId, receipt_id: receiptId }, throwOnError: true })
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: mailKeys.receipts(workspaceId) }),
  })
}

// Admin (root user)

/** The instance switches: workspaces can add mail domains, and their own providers. An absent field stays. */
export function useSetAdminMail() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: SetMailAdminBody) =>
      required((await setMailAdmin({ client: apiClient, body, throwOnError: true })).data),
    onSuccess: (admin) => {
      queryClient.setQueryData(queryKeys.adminMail, admin)
      void queryClient.invalidateQueries({ queryKey: queryKeys.adminSettings })
    },
  })
}

// Providers (instance: root user; workspace: owners and admins)

function useProviderInvalidation(scope: ProviderScope) {
  const queryClient = useQueryClient()
  const workspaceId = useOptionalWorkspaceId()
  return () => {
    void queryClient.invalidateQueries({ queryKey: scope === 'instance' ? queryKeys.adminMailProviders : mailKeys.providers(workspaceId) })
    // a workspace lists the instance providers too, and domains show their provider
    void queryClient.invalidateQueries({ queryKey: mailKeys.all(workspaceId) })
  }
}

/** The created provider's `generated_secrets` and full webhook URL are in this response only. */
export function useCreateProvider(scope: ProviderScope) {
  const workspaceId = useOptionalWorkspaceId()
  const invalidate = useProviderInvalidation(scope)
  return useMutation({
    mutationFn: async (body: CreateMailProviderBody) =>
      required(
        scope === 'instance'
          ? (await createInstanceMailProvider({ client: apiClient, body, throwOnError: true })).data
          : (await createWorkspaceMailProvider({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })).data,
      ),
    onSettled: invalidate,
  })
}

export function useUpdateProvider(scope: ProviderScope) {
  const workspaceId = useOptionalWorkspaceId()
  const invalidate = useProviderInvalidation(scope)
  return useMutation({
    mutationFn: async ({ providerId, body }: { providerId: string; body: UpdateMailProviderBody }) =>
      required(
        scope === 'instance'
          ? (await updateInstanceMailProvider({ client: apiClient, path: { provider_id: providerId }, body, throwOnError: true })).data
          : (
              await updateWorkspaceMailProvider({
                client: apiClient,
                path: { workspace_id: workspaceId, provider_id: providerId },
                body,
                throwOnError: true,
              })
            ).data,
      ),
    onSettled: invalidate,
  })
}

export function useDeleteProvider(scope: ProviderScope) {
  const workspaceId = useOptionalWorkspaceId()
  const invalidate = useProviderInvalidation(scope)
  return useMutation({
    mutationFn: async (providerId: string) => {
      if (scope === 'instance') {
        await deleteInstanceMailProvider({ client: apiClient, path: { provider_id: providerId }, throwOnError: true })
      } else {
        await deleteWorkspaceMailProvider({ client: apiClient, path: { workspace_id: workspaceId, provider_id: providerId }, throwOnError: true })
      }
    },
    onSettled: invalidate,
  })
}

/** A one-time address `check-…@<domain>` (valid 1 h); `useTestAddress(id)` waits for the message. */
export function useCreateTestAddress() {
  return useMutation({
    mutationFn: async (domainId: string) =>
      required((await createMailTestAddress({ client: apiClient, body: { domain_id: domainId }, throwOnError: true })).data),
  })
}
