import { hashKey, type QueryClient, type QueryKey, useMutation, useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useRef, useState } from 'react'
import { apiClient } from '@/api/client'
import {
  addMailboxAlias,
  cancelMailCertificateOrder,
  startMailCertificate,
  verifyMailCertificate,
  addMailboxMember,
  cancelMailOutbound,
  createMailDraft,
  deleteMailDraft,
  deleteMailDraftFile,
  putMailboxVacation,
  retryMailRecipient,
  revokeMailFileLink,
  sendMailDraft,
  updateMailDraft,
  updateMailDraftFile,
  uploadMailDraftFile,
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
  updateMailboxAddress,
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
  createInstanceMailProviderSendRoute,
  createInstanceMailSendRoute,
  createMailSnippet,
  createWorkspaceMailProviderSendRoute,
  createWorkspaceMailSendRoute,
  deleteInstanceMailSendRoute,
  deleteMailSignature,
  deleteMailSnippet,
  deleteMailSuppression,
  deleteWorkspaceMailSendRoute,
  putMailSettings,
  putMailSignature,
  resumeMailSending,
  rotateMailDomainDkim,
  setMailDomainSendRoute,
  setMailSendDefault,
  setMailSendPolicy,
  setMailSendTiers,
  testInstanceMailSendRoute,
  testWorkspaceMailSendRoute,
  updateInstanceMailSendRoute,
  updateMailSnippet,
  updateWorkspaceMailSendRoute,
} from '@orbit/api/generated/sdk.gen'
import type {
  CreateMailboxBody,
  CreateMailDraftBody,
  MailDraft,
  MailDraftFile,
  MailOutbound,
  PutMailVacationBody,
  SendMailDraftBody,
  CreateMailLabelBody,
  CreateMailProviderBody,
  CreatePersonalMailboxesBody,
  MailboxAliasBody,
  MailboxStatus,
  MailCertificate,
  MailCertificateMethod,
  MailSenderPrefBody,
  MailThreadSummary,
  SetMailAdminBody,
  UpdateMailLabelBody,
  UpdateMailProviderBody,
  CreateMailSendRouteBody,
  CreateMailSnippetBody,
  CreateProviderSendRouteBody,
  SetMailSendPolicyBody,
  TierLimits,
  UpdateMailSendRouteBody,
  UpdateMailSnippetBody,
} from '@orbit/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { useContext } from 'react'
import { useWorkspace, WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { ApiProblem } from '@orbit/api/problem'
import { conflictDraft, type DraftText } from '../mailLib'
import { createDraftSaver, type DraftSaver, type DraftSaverState, keepRestore, notifyDraftUndone, onDraftUndone, takeRestore } from './draftSaver'
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
  // the From picker shows the mailbox and address sender names
  mailKeys.fromAddresses(workspaceId).slice(0, 3),
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

/** Sets the sender name of one address; `null` goes back to the mailbox name. */
export function useUpdateMailboxAddress() {
  return useMailboxWrite(
    async (workspaceId, { mailboxId, addressId, senderName }: { mailboxId: string; addressId: string; senderName: string | null }) =>
      required(
        (
          await updateMailboxAddress({
            client: apiClient,
            path: { workspace_id: workspaceId, mailbox_id: mailboxId, address_id: addressId },
            body: { sender_name: senderName },
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
      // the Mail switch shows or hides the mail app for this user at once
      void queryClient.invalidateQueries({ queryKey: queryKeys.authOptions })
    },
  })
}

/** Requests, verifies or cancels the listener's certificate. Each answers with the new state; the health card reloads. */
function useCertificateMutation<T>(request: (input: T) => Promise<MailCertificate | undefined>) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (input: T) => required(await request(input)),
    onSuccess: (certificate) => {
      queryClient.setQueryData(queryKeys.adminMailCertificate, certificate)
      void queryClient.invalidateQueries({ queryKey: queryKeys.adminMail, exact: true })
    },
  })
}

/** `http` validates and issues in the request (up to about a minute); `dns` answers with the TXT record to add. */
export function useStartMailCertificate() {
  return useCertificateMutation(async (method: MailCertificateMethod) =>
    (await startMailCertificate({ client: apiClient, body: { method }, throwOnError: true })).data)
}

export function useVerifyMailCertificate() {
  return useCertificateMutation(async () => (await verifyMailCertificate({ client: apiClient, throwOnError: true })).data)
}

export function useCancelMailCertificateOrder() {
  return useCertificateMutation(async () => (await cancelMailCertificateOrder({ client: apiClient, throwOnError: true })).data)
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

/** A draft changed, was sent or was deleted: the draft lists, the outbound views and the counts are read again. */
const invalidateSendViews = (queryClient: QueryClient, workspaceId: string) =>
  invalidate(queryClient, [mailKeys.draftsAll(workspaceId), mailKeys.outboundAll(workspaceId), mailKeys.counts(workspaceId)])

/** Starts a draft (`new`, `reply`, `reply_all`, `forward`, `forward_attached`). The server answers 200 with the existing one when the thread has a draft already. */
export function useCreateDraft() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationKey: [...mailKeys.all(workspaceId), 'draft'],
    mutationFn: async (body: CreateMailDraftBody) =>
      required((await createMailDraft({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })).data),
    onSuccess: (draft) => {
      queryClient.setQueryData(mailKeys.draft(workspaceId, draft.id), draft)
      void queryClient.invalidateQueries({ queryKey: mailKeys.draftsAll(workspaceId) })
    },
  })
}

export function useDeleteDraft() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationKey: [...mailKeys.all(workspaceId), 'draft'],
    mutationFn: async (draftId: string) => {
      await deleteMailDraft({ client: apiClient, path: { workspace_id: workspaceId, draft_id: draftId }, throwOnError: true })
      return draftId
    },
    onSuccess: (draftId) => {
      queryClient.removeQueries({ queryKey: mailKeys.draft(workspaceId, draftId) })
      void invalidateSendViews(queryClient, workspaceId)
    },
  })
}

/** Changes the files of the cached draft (file changes do not change its `version`). */
function useDraftFilesWrite<Variables>(
  draftId: string,
  send: (workspaceId: string, variables: Variables) => Promise<MailDraftFile | null>,
  apply: (files: MailDraftFile[], result: MailDraftFile | null, variables: Variables) => MailDraftFile[],
) {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: (variables: Variables) => send(workspaceId, variables),
    onSuccess: (result, variables) => {
      queryClient.setQueryData<MailDraft>(mailKeys.draft(workspaceId, draftId), (draft) =>
        draft ? { ...draft, files: apply(draft.files, result, variables) } : draft,
      )
      void queryClient.invalidateQueries({ queryKey: mailKeys.draftsAll(workspaceId) })
    },
  })
}

/**
 * Attaches one file to a draft (413 `upload_too_large`, 422 `draft_files_limit`). `inline`: an image for the text,
 * which gets a `content_id` (422 `inline_not_image` for anything but PNG, JPEG, GIF or WebP).
 */
export function useUploadDraftFile(draftId: string) {
  return useDraftFilesWrite<{ file: File; inline?: boolean }>(
    draftId,
    async (workspaceId, { file, inline }) =>
      required(
        (
          await uploadMailDraftFile({
            client: apiClient,
            path: { workspace_id: workspaceId, draft_id: draftId },
            query: inline ? { inline: true } : undefined,
            body: { file },
            throwOnError: true,
          })
        ).data,
      ),
    (files, file) => (file ? [...files.filter((entry) => entry.id !== file.id), file] : files),
  )
}

export function useRemoveDraftFile(draftId: string) {
  return useDraftFilesWrite<string>(
    draftId,
    async (workspaceId, fileId) => {
      await deleteMailDraftFile({ client: apiClient, path: { workspace_id: workspaceId, draft_id: draftId, file_id: fileId }, throwOnError: true })
      return null
    },
    (files, _result, fileId) => files.filter((file) => file.id !== fileId),
  )
}

/** Sends a file as a download link, or back as an attachment (422 `link_inline`, `link_forwarded`). */
export function useSetDraftFileLink(draftId: string) {
  return useDraftFilesWrite<{ fileId: string; asLink: boolean }>(
    draftId,
    async (workspaceId, { fileId, asLink }) =>
      required(
        (
          await updateMailDraftFile({
            client: apiClient,
            path: { workspace_id: workspaceId, draft_id: draftId, file_id: fileId },
            body: { as_link: asLink },
            throwOnError: true,
          })
        ).data,
      ),
    (files, file) => (file ? files.map((entry) => (entry.id === file.id ? file : entry)) : files),
  )
}

/** The wait before a change is saved: typing makes one save, not one per key. */
export const DRAFT_SAVE_DELAY_MS = 2000

const INITIAL_SAVE_STATE: DraftSaverState = { saving: false, problem: null, restore: null }

/**
 * Debounced autosave of one draft (the state machine is `createDraftSaver`). `save(change)` waits 2 s after the last
 * call; `flush()` saves at once. Every save sends the `version` of the cached draft. On 409 `draft_conflict` the server
 * copy replaces the cached draft and the user's text waits in `restore` (the "Restore my text" banner:
 * `applyRestore()` puts it back as a change, `dismissRestore()` drops it). On 409 `draft_sending` the text goes to
 * `restore` too and saving holds until the cached draft is replaced (Undo). Closing the editor saves what is left once.
 * A draft that is no longer cached (sent) is not saved.
 */
export function useSaveDraft(draftId: string | null | undefined) {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  const [state, setStateValue] = useState<DraftSaverState>(INITIAL_SAVE_STATE)
  // the latest state at once, for code that awaits `flush()` (React state shows it only after a render)
  const latest = useRef<DraftSaverState>(INITIAL_SAVE_STATE)
  const setState = useCallback((next: DraftSaverState) => {
    latest.current = next
    setStateValue(next)
  }, [])
  const saver = useRef<DraftSaver | null>(null)

  useEffect(() => {
    if (!draftId) return
    const key = mailKeys.draft(workspaceId, draftId)
    const hash = hashKey(key)
    const kept = takeRestore(hash)
    setState({ ...INITIAL_SAVE_STATE, restore: kept })
    const instance = createDraftSaver<MailDraft>({
      read: () => queryClient.getQueryData<MailDraft>(key),
      write: (draft) => queryClient.setQueryData(key, draft),
      send: async (change, version) =>
        required(
          (
            await updateMailDraft({
              client: apiClient,
              path: { workspace_id: workspaceId, draft_id: draftId },
              body: { ...change, version },
              throwOnError: true,
            })
          ).data,
        ),
      classify: (error) => {
        const current = conflictDraft<MailDraft>(error)
        if (current) return { kind: 'conflict', current }
        if (error instanceof ApiProblem && error.status === 409 && error.code === 'draft_sending') return { kind: 'sending' }
        return { kind: 'other' }
      },
      onState: setState,
      onSaved: () => void queryClient.invalidateQueries({ queryKey: mailKeys.draftsAll(workspaceId) }),
      setTimer: (run, ms) => setTimeout(run, ms),
      clearTimer: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
      onDispose: (restore) => keepRestore(hash, restore),
      initialRestore: kept,
      delayMs: DRAFT_SAVE_DELAY_MS,
    })
    saver.current = instance
    // Undo returns the draft: a held or stopped saver goes on (nothing else resets it)
    const unsubscribe = onDraftUndone(hash, () => instance.reset())
    return () => {
      unsubscribe()
      instance.dispose()
      if (saver.current === instance) saver.current = null
    }
  }, [draftId, queryClient, workspaceId, setState])

  return {
    ...state,
    /** The state as it is now (after an awaited `flush()`, before React shows it). */
    current: useCallback(() => latest.current, []),
    /** `false`: the change will not be saved (no saver, or it is stopped or held); see `DraftSaver.save`. */
    save: useCallback((change: DraftText) => saver.current?.save(change) ?? false, []),
    flush: useCallback(async () => saver.current?.flush(), []),
    stop: useCallback(() => saver.current?.stop(), []),
    applyRestore: useCallback(() => saver.current?.applyRestore(), []),
    dismissRestore: useCallback(() => saver.current?.dismissRestore(), []),
  }
}

/** Sends a draft: after the undo time, or at `scheduled_at`. The answer says when it goes. */
export function useSendDraft() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation<MailOutbound, Error, { draftId: string; body?: SendMailDraftBody }>({
    mutationKey: [...mailKeys.all(workspaceId), 'send'],
    mutationFn: async ({ draftId, body }) =>
      required(
        (await sendMailDraft({ client: apiClient, path: { workspace_id: workspaceId, draft_id: draftId }, body: body ?? {}, throwOnError: true })).data,
      ),
    onSuccess: (_outbound, { draftId }) => {
      queryClient.removeQueries({ queryKey: mailKeys.draft(workspaceId, draftId) })
      void invalidateSendViews(queryClient, workspaceId)
    },
  })
}

/** **Undo**: stops a message that is held and answers its draft. After its release the server answers 409 `already_sent`. */
export function useCancelOutbound() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationKey: [...mailKeys.all(workspaceId), 'send'],
    mutationFn: async (outboundId: string) =>
      required((await cancelMailOutbound({ client: apiClient, path: { workspace_id: workspaceId, outbound_id: outboundId }, throwOnError: true })).data),
    // the cancelled message is a draft again
    onSuccess: (draft) => {
      const key = mailKeys.draft(workspaceId, draft.id)
      queryClient.setQueryData(key, draft)
      notifyDraftUndone(hashKey(key))
    },
    onSettled: () => invalidateSendViews(queryClient, workspaceId),
  })
}

/** **Retry** one bounced recipient. A purged recipient answers 409 `recipient_expired`. */
export function useRetryRecipient() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationKey: [...mailKeys.all(workspaceId), 'send'],
    mutationFn: async ({ outboundId, recipientId }: { outboundId: string; recipientId: string }) =>
      required(
        (
          await retryMailRecipient({
            client: apiClient,
            path: { workspace_id: workspaceId, outbound_id: outboundId, recipient_id: recipientId },
            throwOnError: true,
          })
        ).data,
      ),
    // the recipient's status shows in the Outbox and on the sent message
    onSettled: () => invalidate(queryClient, [mailKeys.outboundAll(workspaceId), mailKeys.threadDetails(workspaceId)]),
  })
}

/** **Revoke** a file link of a message the user sent: the link stops working at once. */
export function useRevokeFileLink() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async ({ threadId, messageId, linkId }: { threadId: string; messageId: string; linkId: string }) =>
      required(
        (
          await revokeMailFileLink({
            client: apiClient,
            path: { workspace_id: workspaceId, thread_id: threadId, message_id: messageId, link_id: linkId },
            throwOnError: true,
          })
        ).data,
      ),
    onSettled: (_link, _error, { threadId }) => queryClient.invalidateQueries({ queryKey: mailKeys.thread(workspaceId, threadId) }),
  })
}

/** Sets a mailbox's vacation reply (the whole of it: the server replaces every field). */
export function useSetVacation() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async ({ mailboxId, body }: { mailboxId: string; body: PutMailVacationBody }) =>
      required((await putMailboxVacation({ client: apiClient, path: { workspace_id: workspaceId, mailbox_id: mailboxId }, body, throwOnError: true })).data),
    onSuccess: (vacation, { mailboxId }) => queryClient.setQueryData(mailKeys.vacation(workspaceId, mailboxId), vacation),
  })
}

// Sending settings (slice 2)

/**
 * After a route changes: the route list, and in the current workspace the domains (their route, its SPF) and the From
 * addresses (whether they can send, the size limit).
 */
function useSendRouteInvalidation(scope: ProviderScope) {
  const queryClient = useQueryClient()
  const workspaceId = useOptionalWorkspaceId()
  return () => {
    void queryClient.invalidateQueries({ queryKey: scope === 'instance' ? queryKeys.adminMailSendRoutes : mailKeys.sendRoutes(workspaceId) })
    if (!workspaceId) return
    if (scope === 'instance') void queryClient.invalidateQueries({ queryKey: mailKeys.sendRoutes(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: mailKeys.domains(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: mailKeys.fromAddresses(workspaceId).slice(0, 3) })
  }
}

export function useCreateSendRoute(scope: ProviderScope) {
  const workspaceId = useOptionalWorkspaceId()
  const invalidate = useSendRouteInvalidation(scope)
  return useMutation({
    mutationFn: async (body: CreateMailSendRouteBody) =>
      required(
        scope === 'instance'
          ? (await createInstanceMailSendRoute({ client: apiClient, body, throwOnError: true })).data
          : (await createWorkspaceMailSendRoute({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })).data,
      ),
    onSettled: invalidate,
  })
}

/** Secrets left out of `body.secrets` keep their values; `body.settings` replaces the settings. */
export function useUpdateSendRoute(scope: ProviderScope) {
  const workspaceId = useOptionalWorkspaceId()
  const invalidate = useSendRouteInvalidation(scope)
  return useMutation({
    mutationFn: async ({ routeId, body }: { routeId: string; body: UpdateMailSendRouteBody }) =>
      required(
        scope === 'instance'
          ? (await updateInstanceMailSendRoute({ client: apiClient, path: { route_id: routeId }, body, throwOnError: true })).data
          : (await updateWorkspaceMailSendRoute({ client: apiClient, path: { workspace_id: workspaceId, route_id: routeId }, body, throwOnError: true })).data,
      ),
    onSettled: invalidate,
  })
}

/** Refused while a domain uses the route (`route_in_use`) or while it is the default (`route_is_default`). */
export function useDeleteSendRoute(scope: ProviderScope) {
  const workspaceId = useOptionalWorkspaceId()
  const invalidate = useSendRouteInvalidation(scope)
  return useMutation({
    mutationFn: async (routeId: string) => {
      if (scope === 'instance') {
        await deleteInstanceMailSendRoute({ client: apiClient, path: { route_id: routeId }, throwOnError: true })
      } else {
        await deleteWorkspaceMailSendRoute({ client: apiClient, path: { workspace_id: workspaceId, route_id: routeId }, throwOnError: true })
      }
    },
    onSettled: invalidate,
  })
}

/** **Send test**: a message to `to` through the route, with the result of each step. */
export function useTestSendRoute(scope: ProviderScope) {
  const queryClient = useQueryClient()
  const workspaceId = useOptionalWorkspaceId()
  return useMutation({
    mutationFn: async ({ routeId, to }: { routeId: string; to: string }) =>
      required(
        scope === 'instance'
          ? (await testInstanceMailSendRoute({ client: apiClient, path: { route_id: routeId }, body: { to }, throwOnError: true })).data
          : (await testWorkspaceMailSendRoute({ client: apiClient, path: { workspace_id: workspaceId, route_id: routeId }, body: { to }, throwOnError: true })).data,
      ),
    // the route's last send or error changed
    onSettled: () => queryClient.invalidateQueries({ queryKey: scope === 'instance' ? queryKeys.adminMailSendRoutes : mailKeys.sendRoutes(workspaceId) }),
  })
}

/** **Also send through this provider**: a route with the provider's account and only what it lacks. */
export function useCreateProviderSendRoute(scope: ProviderScope) {
  const workspaceId = useOptionalWorkspaceId()
  const invalidate = useSendRouteInvalidation(scope)
  return useMutation({
    mutationFn: async ({ providerId, body }: { providerId: string; body: CreateProviderSendRouteBody }) =>
      required(
        scope === 'instance'
          ? (await createInstanceMailProviderSendRoute({ client: apiClient, path: { provider_id: providerId }, body, throwOnError: true })).data
          : (
              await createWorkspaceMailProviderSendRoute({
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

/** The instance route that domains without one use; `null` clears it (root user). */
export function useSetSendDefault() {
  const queryClient = useQueryClient()
  const workspaceId = useOptionalWorkspaceId()
  // the domains of the current workspace that follow the default now send through another route
  const invalidate = () => {
    if (!workspaceId) return
    void queryClient.invalidateQueries({ queryKey: mailKeys.sendRoutes(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: mailKeys.domains(workspaceId) })
    void queryClient.invalidateQueries({ queryKey: mailKeys.fromAddresses(workspaceId).slice(0, 3) })
  }
  return useMutation({
    mutationFn: async (routeId: string | null) =>
      required((await setMailSendDefault({ client: apiClient, body: { route_id: routeId }, throwOnError: true })).data),
    onSuccess: (list) => queryClient.setQueryData(queryKeys.adminMailSendRoutes, list),
    onSettled: invalidate,
  })
}

/** A domain's send route: one of the workspace or an instance route; `null` uses the instance default. */
export function useSetDomainSendRoute() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async ({ domainId, routeId }: { domainId: string; routeId: string | null }) =>
      required(
        (
          await setMailDomainSendRoute({
            client: apiClient,
            path: { workspace_id: workspaceId, domain_id: domainId },
            body: { route_id: routeId },
            throwOnError: true,
          })
        ).data,
      ),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: mailKeys.domains(workspaceId) })
      void queryClient.invalidateQueries({ queryKey: mailKeys.sendRoutes(workspaceId) })
      // the From picker says whether an address can send
      void queryClient.invalidateQueries({ queryKey: mailKeys.fromAddresses(workspaceId).slice(0, 3) })
    },
  })
}

/** Starts a DKIM key rotation; the domain comes back with the new record to publish (`409 rotation_pending`). */
export function useRotateDkim() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async (domainId: string) =>
      required((await rotateMailDomainDkim({ client: apiClient, path: { workspace_id: workspaceId, domain_id: domainId }, throwOnError: true })).data),
    onSuccess: (domain) => queryClient.setQueryData(mailKeys.domain(workspaceId, domain.id), domain),
    onSettled: () => queryClient.invalidateQueries({ queryKey: mailKeys.domains(workspaceId), exact: true }),
  })
}

/** Takes an address off the suppression list: mail to it is sent again. */
export function useDeleteSuppression() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async (address: string) => {
      await deleteMailSuppression({ client: apiClient, path: { workspace_id: workspaceId, address }, throwOnError: true })
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: mailKeys.suppressions(workspaceId) }),
  })
}

/** The signature of one address the user sends as; an empty body removes it. */
export function useSaveSignature() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async ({ addressId, bodyHtml }: { addressId: string; bodyHtml: string }) => {
      if (bodyHtml.trim() === '') {
        await deleteMailSignature({ client: apiClient, path: { workspace_id: workspaceId, address_id: addressId }, throwOnError: true })
        return { address_id: addressId, body_html: null }
      }
      return required(
        (await putMailSignature({ client: apiClient, path: { workspace_id: workspaceId, address_id: addressId }, body: { body_html: bodyHtml }, throwOnError: true }))
          .data,
      )
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: mailKeys.signatures(workspaceId) })
      // the composer reads each address's signature with the From addresses
      void queryClient.invalidateQueries({ queryKey: mailKeys.fromAddresses(workspaceId).slice(0, 3) })
    },
  })
}

/** The member's undo send delay (0, 5, 10, 20 or 30 s). */
export function useSaveMailSettings() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async (undoSendSeconds: number) =>
      required((await putMailSettings({ client: apiClient, path: { workspace_id: workspaceId }, body: { undo_send_seconds: undoSendSeconds }, throwOnError: true })).data),
    onSuccess: (settings) => queryClient.setQueryData(mailKeys.settingsMe(workspaceId), settings),
  })
}

export function useCreateSnippet() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async (body: CreateMailSnippetBody) =>
      required((await createMailSnippet({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })).data),
    onSettled: () => queryClient.invalidateQueries({ queryKey: mailKeys.snippets(workspaceId) }),
  })
}

export function useUpdateSnippet() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async ({ snippetId, body }: { snippetId: string; body: UpdateMailSnippetBody }) =>
      required((await updateMailSnippet({ client: apiClient, path: { workspace_id: workspaceId, snippet_id: snippetId }, body, throwOnError: true })).data),
    onSettled: () => queryClient.invalidateQueries({ queryKey: mailKeys.snippets(workspaceId) }),
  })
}

export function useDeleteSnippet() {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspaceId()
  return useMutation({
    mutationFn: async (snippetId: string) => {
      await deleteMailSnippet({ client: apiClient, path: { workspace_id: workspaceId, snippet_id: snippetId }, throwOnError: true })
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: mailKeys.snippets(workspaceId) }),
  })
}

// Send policies (root user)

const invalidatePolicies = (queryClient: QueryClient) => queryClient.invalidateQueries({ queryKey: queryKeys.adminMailSendPolicies })

/** The limits of the New and Standard tiers; workspaces on those tiers use them at once. */
export function useSetSendTiers() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: TierLimits) => required((await setMailSendTiers({ client: apiClient, body, throwOnError: true })).data),
    onSuccess: (tiers) => queryClient.setQueryData(queryKeys.adminMailSendTiers, tiers),
    onSettled: () => invalidatePolicies(queryClient),
  })
}

/** A workspace's tier, and for `custom` its limits. */
export function useSetSendPolicy() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ workspaceId, body }: { workspaceId: string; body: SetMailSendPolicyBody }) =>
      required((await setMailSendPolicy({ client: apiClient, path: { workspace_id: workspaceId }, body, throwOnError: true })).data),
    onSettled: () => invalidatePolicies(queryClient),
  })
}

/** **Resume**: a paused workspace sends again; its held messages go at the next sweep. */
export function useResumeSending() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (workspaceId: string) =>
      required((await resumeMailSending({ client: apiClient, path: { workspace_id: workspaceId }, throwOnError: true })).data),
    onSettled: () => invalidatePolicies(queryClient),
  })
}
